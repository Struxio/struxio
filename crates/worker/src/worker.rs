use std::time::Duration;
use struxio_core::queue::{ExtractionJob, QueueConsumer, QueueError, RetryClass, RetryRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    Retryable,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessingFailure {
    pub class: FailureClass,
    pub reason: String,
}

impl ProcessingFailure {
    pub fn retryable(reason: impl Into<String>) -> Self {
        Self {
            class: FailureClass::Retryable,
            reason: reason.into(),
        }
    }

    pub fn permanent(reason: impl Into<String>) -> Self {
        Self {
            class: FailureClass::Permanent,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub initial_delay: Duration,
    pub max_delay: Duration,
}

impl RetryPolicy {
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(31);
        let multiplier = 1u32 << exponent;
        self.initial_delay
            .checked_mul(multiplier)
            .unwrap_or(self.max_delay)
            .min(self.max_delay)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    Ack,
    Retry { delay: Duration, reason: String },
    DeadLetter { reason: String },
}

pub fn settlement_for(
    job: &ExtractionJob,
    failure: Option<&ProcessingFailure>,
    policy: RetryPolicy,
) -> Settlement {
    let Some(failure) = failure else {
        return Settlement::Ack;
    };

    if failure.class == FailureClass::Retryable && job.attempt < job.max_attempts {
        Settlement::Retry {
            delay: policy.delay_for(job.attempt),
            reason: failure.reason.clone(),
        }
    } else {
        Settlement::DeadLetter {
            reason: failure.reason.clone(),
        }
    }
}

/// Apply the queue side of a settlement. The caller must perform the
/// Postgres terminal/retry update before invoking this function.
pub async fn apply_settlement<C: QueueConsumer>(
    consumer: &C,
    job: &ExtractionJob,
    settlement: Settlement,
) -> Result<(), QueueError> {
    match settlement {
        Settlement::Ack => consumer.ack(&job.stream_id).await,
        Settlement::Retry { delay, reason } => {
            consumer
                .retry(
                    job,
                    RetryRequest {
                        delay,
                        class: RetryClass::Transient,
                        reason,
                    },
                )
                .await
        }
        Settlement::DeadLetter { reason } => consumer.dead_letter(job, &reason).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use struxio_common::WorkspaceId;
    use struxio_core::queue::{QueueConsumer, RetryRequest};
    use uuid::Uuid;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Action {
        Ack(String),
        Retry(u32, Duration),
        DeadLetter(String),
    }

    #[derive(Clone, Default)]
    struct FakeQueue {
        actions: Arc<Mutex<Vec<Action>>>,
    }

    #[async_trait]
    impl QueueConsumer for FakeQueue {
        async fn ensure_group(&self) -> Result<(), QueueError> {
            Ok(())
        }

        async fn next_job(&self) -> Result<Option<ExtractionJob>, QueueError> {
            Ok(None)
        }

        async fn ack(&self, stream_id: &str) -> Result<(), QueueError> {
            self.actions
                .lock()
                .unwrap()
                .push(Action::Ack(stream_id.to_string()));
            Ok(())
        }

        async fn retry(
            &self,
            job: &ExtractionJob,
            request: RetryRequest,
        ) -> Result<(), QueueError> {
            self.actions
                .lock()
                .unwrap()
                .push(Action::Retry(job.attempt + 1, request.delay));
            Ok(())
        }

        async fn dead_letter(&self, _job: &ExtractionJob, reason: &str) -> Result<(), QueueError> {
            self.actions
                .lock()
                .unwrap()
                .push(Action::DeadLetter(reason.to_string()));
            Ok(())
        }
    }

    fn job(attempt: u32, max_attempts: u32) -> ExtractionJob {
        let mut job = ExtractionJob::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            WorkspaceId::local(),
            None,
            max_attempts,
        );
        job.stream_id = "1-0".to_string();
        job.attempt = attempt;
        job
    }

    #[test]
    fn retry_delay_is_capped_exponential_backoff() {
        let policy = RetryPolicy {
            initial_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(5),
        };
        assert_eq!(policy.delay_for(1), Duration::from_secs(2));
        assert_eq!(policy.delay_for(2), Duration::from_secs(4));
        assert_eq!(policy.delay_for(3), Duration::from_secs(5));
    }

    #[test]
    fn permanent_and_exhausted_failures_go_to_dlq() {
        let policy = RetryPolicy {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
        };
        let permanent = ProcessingFailure::permanent("bad template");
        assert_eq!(
            settlement_for(&job(1, 5), Some(&permanent), policy),
            Settlement::DeadLetter {
                reason: "bad template".to_string()
            }
        );
        let transient = ProcessingFailure::retryable("timeout");
        assert!(matches!(
            settlement_for(&job(5, 5), Some(&transient), policy),
            Settlement::DeadLetter { .. }
        ));
    }

    #[tokio::test]
    async fn fake_queue_applies_retry_without_ack_before_it() {
        let queue = FakeQueue::default();
        let job = job(1, 3);
        let settlement = settlement_for(
            &job,
            Some(&ProcessingFailure::retryable("provider timeout")),
            RetryPolicy {
                initial_delay: Duration::from_secs(1),
                max_delay: Duration::from_secs(10),
            },
        );
        apply_settlement(&queue, &job, settlement).await.unwrap();
        assert_eq!(
            *queue.actions.lock().unwrap(),
            vec![Action::Retry(2, Duration::from_secs(1))]
        );
    }
}
