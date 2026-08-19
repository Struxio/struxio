// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::time::Duration;

use struxio_common::models::BatchStatus;
use uuid::Uuid;

use super::envelope::JobEnvelope;
use super::error::{retryable_http_status, JobError};
use super::retry::{RetryPolicy, Retryability};
use super::state::{event_for_error, step, DeliveryAction, JobEvent, JobSnapshot, JobStatus};
use super::ChildCounts;

fn policy() -> RetryPolicy {
    RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_millis(800),
        0.25,
    )
}

fn walk(events: &[JobEvent]) -> (JobSnapshot, Vec<DeliveryAction>) {
    let mut snap = JobSnapshot::pending();
    let mut actions = Vec::new();
    for event in events {
        let decision = step(snap, *event, policy(), 0.5);
        snap = decision.snapshot;
        actions.push(decision.action);
    }
    (snap, actions)
}

#[test]
fn pending_delivery_claims_attempt_one_and_executes() {
    let decision = step(JobSnapshot::pending(), JobEvent::Deliver, policy(), 0.5);
    assert_eq!(decision.snapshot.status, JobStatus::Processing);
    assert_eq!(decision.snapshot.attempt, 1);
    assert_eq!(decision.action, DeliveryAction::Execute);
}

#[test]
fn processing_reclaim_does_not_burn_an_attempt() {
    let current = JobSnapshot {
        status: JobStatus::Processing,
        attempt: 1,
    };
    let decision = step(current, JobEvent::Deliver, policy(), 0.5);
    assert_eq!(decision.snapshot.attempt, 1);
    assert_eq!(decision.action, DeliveryAction::Execute);
}

#[test]
fn success_is_acked_and_becomes_completed() {
    let current = JobSnapshot {
        status: JobStatus::Processing,
        attempt: 1,
    };
    let decision = step(current, JobEvent::Succeed, policy(), 0.5);
    assert_eq!(decision.snapshot.status, JobStatus::Completed);
    assert_eq!(decision.action, DeliveryAction::AckComplete);
}

#[test]
fn retryable_failure_schedules_retry_before_max_attempts() {
    let current = JobSnapshot {
        status: JobStatus::Processing,
        attempt: 1,
    };
    let decision = step(current, JobEvent::FailRetryable, policy(), 0.5);
    assert_eq!(decision.snapshot.status, JobStatus::Retrying);
    assert_eq!(decision.snapshot.attempt, 1);
    match decision.action {
        DeliveryAction::AckRetry { delay } => assert_eq!(delay, Duration::from_millis(100)),
        other => panic!("expected retry, got {other:?}"),
    }
}

#[test]
fn retryable_failure_dead_letters_when_attempts_exhausted() {
    let current = JobSnapshot {
        status: JobStatus::Processing,
        attempt: 3,
    };
    let decision = step(current, JobEvent::FailRetryable, policy(), 0.5);
    assert_eq!(decision.snapshot.status, JobStatus::Failed);
    assert_eq!(decision.action, DeliveryAction::AckDeadLetter);
}

#[test]
fn permanent_failure_dead_letters_on_first_attempt() {
    let current = JobSnapshot {
        status: JobStatus::Processing,
        attempt: 1,
    };
    let decision = step(current, JobEvent::FailPermanent, policy(), 0.5);
    assert_eq!(decision.snapshot.status, JobStatus::Failed);
    assert_eq!(decision.action, DeliveryAction::AckDeadLetter);
}

#[test]
fn terminal_redelivery_is_idempotent_skip() {
    for status in [JobStatus::Completed, JobStatus::Failed] {
        let current = JobSnapshot { status, attempt: 2 };
        let decision = step(current, JobEvent::Deliver, policy(), 0.5);
        assert_eq!(decision.snapshot, current);
        assert_eq!(decision.action, DeliveryAction::AckSkip);

        let succeed = step(current, JobEvent::Succeed, policy(), 0.5);
        assert_eq!(succeed.action, DeliveryAction::AckSkip);
        assert_eq!(succeed.snapshot.status, status);

        let fail = step(current, JobEvent::FailRetryable, policy(), 0.5);
        assert_eq!(fail.action, DeliveryAction::AckSkip);
        assert_eq!(fail.snapshot.status, status);
    }
}

#[test]
fn malformed_payload_is_dead_lettered_without_running() {
    let decision = step(JobSnapshot::pending(), JobEvent::Malformed, policy(), 0.5);
    assert_eq!(decision.snapshot.status, JobStatus::Pending);
    assert_eq!(decision.action, DeliveryAction::AckMalformed);
}

#[test]
fn retry_then_success_path() {
    let (snap, actions) = walk(&[
        JobEvent::Deliver,
        JobEvent::FailRetryable,
        JobEvent::Deliver,
        JobEvent::Succeed,
    ]);
    assert_eq!(snap.status, JobStatus::Completed);
    assert_eq!(snap.attempt, 2);
    assert_eq!(
        actions,
        vec![
            DeliveryAction::Execute,
            DeliveryAction::AckRetry {
                delay: Duration::from_millis(100)
            },
            DeliveryAction::Execute,
            DeliveryAction::AckComplete,
        ]
    );
}

#[test]
fn retry_then_exhaustion_path() {
    let (snap, actions) = walk(&[
        JobEvent::Deliver,
        JobEvent::FailRetryable,
        JobEvent::Deliver,
        JobEvent::FailRetryable,
        JobEvent::Deliver,
        JobEvent::FailRetryable,
    ]);
    assert_eq!(snap.status, JobStatus::Failed);
    assert_eq!(snap.attempt, 3);
    assert_eq!(actions.last(), Some(&DeliveryAction::AckDeadLetter));
    assert!(
        actions
            .iter()
            .filter(|a| **a == DeliveryAction::Execute)
            .count()
            == 3
    );
}

#[test]
fn backoff_is_exponential_and_capped_without_jitter() {
    let policy = policy();
    assert_eq!(policy.backoff(1, 0.5), Duration::from_millis(100));
    assert_eq!(policy.backoff(2, 0.5), Duration::from_millis(200));
    assert_eq!(policy.backoff(3, 0.5), Duration::from_millis(400));
    assert_eq!(policy.backoff(4, 0.5), Duration::from_millis(800));
    assert_eq!(policy.backoff(8, 0.5), Duration::from_millis(800));
}

#[test]
fn backoff_jitter_stays_within_configured_ratio() {
    let policy = policy();
    let low = policy.backoff(1, 0.0);
    let high = policy.backoff(1, 1.0);
    assert_eq!(low, Duration::from_millis(75));
    assert_eq!(high, Duration::from_millis(125));
}

#[test]
fn retryability_is_explicit_on_job_errors() {
    let retry = JobError::retryable("429 from provider");
    let permanent = JobError::permanent("document not found");
    assert_eq!(retry.retryability(), Retryability::Retryable);
    assert_eq!(permanent.retryability(), Retryability::Permanent);
    assert_eq!(event_for_error(&retry), JobEvent::FailRetryable);
    assert_eq!(event_for_error(&permanent), JobEvent::FailPermanent);
}

#[test]
fn provider_http_status_retryability() {
    for status in [408, 429, 500, 502, 503, 504] {
        assert!(retryable_http_status(status), "{status}");
    }
    for status in [400, 401, 403, 404, 422] {
        assert!(!retryable_http_status(status), "{status}");
    }
}

#[test]
fn batch_stays_processing_while_any_child_is_retrying() {
    let counts = ChildCounts {
        pending: 0,
        processing: 0,
        retrying: 1,
        completed: 2,
        failed: 1,
    };
    assert_eq!(counts.batch_status(), BatchStatus::Processing);
    assert!(!counts.is_terminal());
}

#[test]
fn batch_terminal_states_are_honest() {
    assert_eq!(
        ChildCounts {
            pending: 3,
            processing: 0,
            retrying: 0,
            completed: 0,
            failed: 0,
        }
        .batch_status(),
        BatchStatus::Pending
    );
    assert_eq!(
        ChildCounts {
            pending: 0,
            processing: 1,
            retrying: 0,
            completed: 1,
            failed: 0,
        }
        .batch_status(),
        BatchStatus::Processing
    );
    assert_eq!(
        ChildCounts {
            pending: 0,
            processing: 0,
            retrying: 0,
            completed: 3,
            failed: 0,
        }
        .batch_status(),
        BatchStatus::Completed
    );
    assert_eq!(
        ChildCounts {
            pending: 0,
            processing: 0,
            retrying: 0,
            completed: 2,
            failed: 1,
        }
        .batch_status(),
        BatchStatus::PartiallyCompleted
    );
    assert_eq!(
        ChildCounts {
            pending: 0,
            processing: 0,
            retrying: 0,
            completed: 0,
            failed: 3,
        }
        .batch_status(),
        BatchStatus::Failed
    );
}

#[test]
fn envelope_requires_workspace_and_round_trips() {
    let envelope = JobEnvelope {
        extraction_id: Uuid::new_v4(),
        document_id: Uuid::new_v4(),
        template_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        batch_job_id: Some(Uuid::new_v4()),
    };
    let fields: HashMap<_, _> = envelope.to_fields().into_iter().collect();
    let parsed = JobEnvelope::from_fields(&fields).expect("parse");
    assert_eq!(parsed, envelope);

    let delayed = envelope.to_delayed_payload().unwrap();
    assert_eq!(
        JobEnvelope::from_delayed_payload(&delayed).unwrap(),
        envelope
    );
}

#[test]
fn envelope_rejects_missing_or_nil_workspace() {
    let mut fields = HashMap::new();
    fields.insert("extraction_id".into(), Uuid::new_v4().to_string());
    fields.insert("document_id".into(), Uuid::new_v4().to_string());
    fields.insert("template_id".into(), Uuid::new_v4().to_string());
    assert!(JobEnvelope::from_fields(&fields).is_err());

    fields.insert("workspace_id".into(), Uuid::nil().to_string());
    let err = JobEnvelope::from_fields(&fields).unwrap_err();
    assert_eq!(err.retryability(), Retryability::Permanent);
}

#[test]
fn counters_do_not_change_on_skip_of_already_terminal_job() {
    // Model the batch-counter rule: only Complete/DeadLetter after a
    // non-terminal snapshot may bump counts. Skip must not.
    let before = ChildCounts {
        pending: 0,
        processing: 0,
        retrying: 0,
        completed: 1,
        failed: 0,
    };
    let decision = step(
        JobSnapshot {
            status: JobStatus::Completed,
            attempt: 1,
        },
        JobEvent::Succeed,
        policy(),
        0.5,
    );
    assert_eq!(decision.action, DeliveryAction::AckSkip);
    assert_eq!(before.completed, 1);
    assert_eq!(before.failed, 0);
}
