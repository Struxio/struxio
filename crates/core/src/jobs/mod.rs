// SPDX-License-Identifier: AGPL-3.0-only

//! Durable job delivery policy for Redis Stream extraction work.
//!
//! This module is side-effect free: it classifies retryability, computes
//! backoff, and reduces job/batch state. Redis and Postgres apply the
//! resulting [`DeliveryAction`].

pub mod envelope;
pub mod error;
pub mod retry;
pub mod state;

pub use envelope::{JobEnvelope, CONSUMER_GROUP, DELAYED_ZSET, DLQ_STREAM, READY_STREAM};
pub use error::{retryable_http_status, JobError};
pub use retry::{RetryPolicy, Retryability};
pub use state::{
    event_for_error, step, DeliveryAction, DeliveryDecision, JobEvent, JobSnapshot, JobStatus,
};
pub use struxio_common::models::ChildCounts;

#[cfg(test)]
mod tests;
