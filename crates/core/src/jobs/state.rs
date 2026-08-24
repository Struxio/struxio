// SPDX-License-Identifier: AGPL-3.0-only

use std::time::Duration;

use super::error::JobError;
use super::retry::{RetryPolicy, Retryability};

/// Durable job lifecycle stored on the extraction row (and mirrored in tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Processing,
    Retrying,
    Completed,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Retrying => "retrying",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "processing" => Some(Self::Processing),
            "retrying" => Some(Self::Retrying),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }

    pub fn is_open(self) -> bool {
        !self.is_terminal()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobSnapshot {
    pub status: JobStatus,
    pub attempt: u32,
}

impl JobSnapshot {
    pub fn pending() -> Self {
        Self {
            status: JobStatus::Pending,
            attempt: 0,
        }
    }
}

/// What the worker should do with the Redis Stream entry after applying
/// durable Postgres state for this event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryAction {
    /// Run the handler. Do not ACK yet.
    Execute,
    /// Already terminal: ACK the stream entry, do not change counters.
    AckSkip,
    /// Durable success recorded: ACK.
    AckComplete,
    /// Durable retry recorded (delayed set / status=retrying): ACK the current
    /// entry. A new entry will be promoted after `delay`.
    AckRetry { delay: Duration },
    /// Durable failure recorded and copied to the DLQ: ACK.
    AckDeadLetter,
    /// Malformed payload: copy to the DLQ and ACK. Never execute.
    AckMalformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobEvent {
    /// A stream entry was delivered (new, delayed, or reclaimed).
    Deliver,
    Succeed,
    FailRetryable,
    FailPermanent,
    Malformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryDecision {
    pub snapshot: JobSnapshot,
    pub action: DeliveryAction,
}

/// Pure reducer for job + Redis ACK state. Database and Redis I/O apply
/// `action` after this returns; they are not part of the reducer.
pub fn step(
    current: JobSnapshot,
    event: JobEvent,
    policy: RetryPolicy,
    jitter_unit: f64,
) -> DeliveryDecision {
    match event {
        JobEvent::Malformed => DeliveryDecision {
            snapshot: current,
            action: DeliveryAction::AckMalformed,
        },
        JobEvent::Deliver => match current.status {
            JobStatus::Completed | JobStatus::Failed => DeliveryDecision {
                snapshot: current,
                action: DeliveryAction::AckSkip,
            },
            JobStatus::Pending | JobStatus::Retrying => {
                let attempt = current.attempt.saturating_add(1);
                if attempt > policy.max_attempts {
                    DeliveryDecision {
                        snapshot: JobSnapshot {
                            status: JobStatus::Failed,
                            attempt,
                        },
                        action: DeliveryAction::AckDeadLetter,
                    }
                } else {
                    DeliveryDecision {
                        snapshot: JobSnapshot {
                            status: JobStatus::Processing,
                            attempt,
                        },
                        action: DeliveryAction::Execute,
                    }
                }
            }
            JobStatus::Processing => {
                // Crash reclaim of the same attempt: do not burn another try.
                if current.attempt == 0 {
                    DeliveryDecision {
                        snapshot: JobSnapshot {
                            status: JobStatus::Processing,
                            attempt: 1,
                        },
                        action: DeliveryAction::Execute,
                    }
                } else if current.attempt > policy.max_attempts {
                    DeliveryDecision {
                        snapshot: JobSnapshot {
                            status: JobStatus::Failed,
                            attempt: current.attempt,
                        },
                        action: DeliveryAction::AckDeadLetter,
                    }
                } else {
                    DeliveryDecision {
                        snapshot: current,
                        action: DeliveryAction::Execute,
                    }
                }
            }
        },
        JobEvent::Succeed => {
            if current.status.is_terminal() {
                DeliveryDecision {
                    snapshot: current,
                    action: DeliveryAction::AckSkip,
                }
            } else {
                DeliveryDecision {
                    snapshot: JobSnapshot {
                        status: JobStatus::Completed,
                        attempt: current.attempt,
                    },
                    action: DeliveryAction::AckComplete,
                }
            }
        }
        JobEvent::FailPermanent => {
            if current.status.is_terminal() {
                DeliveryDecision {
                    snapshot: current,
                    action: DeliveryAction::AckSkip,
                }
            } else {
                DeliveryDecision {
                    snapshot: JobSnapshot {
                        status: JobStatus::Failed,
                        attempt: current.attempt,
                    },
                    action: DeliveryAction::AckDeadLetter,
                }
            }
        }
        JobEvent::FailRetryable => {
            if current.status.is_terminal() {
                DeliveryDecision {
                    snapshot: current,
                    action: DeliveryAction::AckSkip,
                }
            } else if policy.can_retry(current.attempt) {
                DeliveryDecision {
                    snapshot: JobSnapshot {
                        status: JobStatus::Retrying,
                        attempt: current.attempt,
                    },
                    action: DeliveryAction::AckRetry {
                        delay: policy.backoff(current.attempt, jitter_unit),
                    },
                }
            } else {
                DeliveryDecision {
                    snapshot: JobSnapshot {
                        status: JobStatus::Failed,
                        attempt: current.attempt,
                    },
                    action: DeliveryAction::AckDeadLetter,
                }
            }
        }
    }
}

pub fn event_for_error(error: &JobError) -> JobEvent {
    match error.retryability() {
        Retryability::Retryable => JobEvent::FailRetryable,
        Retryability::Permanent => JobEvent::FailPermanent,
    }
}
