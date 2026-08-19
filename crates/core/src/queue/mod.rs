use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use struxio_common::WorkspaceId;
use uuid::Uuid;

pub mod redis;

// ── Shared error type ───────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("redis error: {0}")]
    Redis(#[from] ::redis::RedisError),
    #[error("invalid queue message {stream_id}: {reason}")]
    InvalidMessage { stream_id: String, reason: String },
    #[error("queue error: {0}")]
    Other(String),
}

// ── Shared job type ─────────────────────────────────────────────────────────

pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionJob {
    /// Opaque message identifier (e.g. Redis stream ID, Kafka offset string).
    /// Used only for acknowledgement — callers should treat it as opaque.
    pub stream_id: String,
    /// Stable application-level identity. It is intentionally independent of
    /// the Redis stream entry ID so retries and redeliveries remain idempotent.
    pub job_id: Uuid,
    pub extraction_id: Uuid,
    pub document_id: Uuid,
    pub template_id: Uuid,
    pub workspace_id: WorkspaceId,
    pub batch_job_id: Option<Uuid>,
    /// One-based delivery attempt, carried in every retry payload.
    pub attempt: u32,
    /// Maximum number of processing attempts, carried in the original job.
    pub max_attempts: u32,
}

impl ExtractionJob {
    pub fn new(
        extraction_id: Uuid,
        document_id: Uuid,
        template_id: Uuid,
        workspace_id: WorkspaceId,
        batch_job_id: Option<Uuid>,
        max_attempts: u32,
    ) -> Self {
        Self {
            stream_id: String::new(),
            job_id: extraction_id,
            extraction_id,
            document_id,
            template_id,
            workspace_id,
            batch_job_id,
            attempt: 1,
            max_attempts: max_attempts.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    Transient,
    Permanent,
}

#[derive(Debug, Clone)]
pub struct RetryRequest {
    pub delay: Duration,
    pub class: RetryClass,
    pub reason: String,
}

// ── Traits ──────────────────────────────────────────────────────────────────

/// Capability to push extraction jobs onto the queue.
///
/// Implement this for each broker backend (Redis Streams, Kafka, …).
/// The bound `Clone + Send + Sync + 'static` allows it to be stored in
/// `Arc`-backed `AppState` and cloned cheaply across requests.
#[async_trait]
pub trait QueueProducer: Clone + Send + Sync + 'static {
    async fn enqueue_extraction(
        &self,
        extraction_id: Uuid,
        document_id: Uuid,
        template_id: Uuid,
        workspace_id: WorkspaceId,
        batch_job_id: Option<Uuid>,
    ) -> Result<(), QueueError>;
}

/// Capability to consume extraction jobs from the queue.
///
/// Implement this for each broker backend.
#[async_trait]
pub trait QueueConsumer: Send + Sync + 'static {
    /// Ensure the consumer group / subscription exists (idempotent).
    async fn ensure_group(&self) -> Result<(), QueueError>;
    /// Poll for the next available job. Returns `None` on timeout with no job.
    async fn next_job(&self) -> Result<Option<ExtractionJob>, QueueError>;
    /// Acknowledge only after the application has durably recorded a terminal
    /// result or a retry/DLQ decision.
    async fn ack(&self, stream_id: &str) -> Result<(), QueueError>;
    /// Atomically persist a scheduled retry and acknowledge the current entry.
    async fn retry(&self, job: &ExtractionJob, request: RetryRequest) -> Result<(), QueueError>;
    /// Persist a DLQ entry and acknowledge the current entry atomically.
    async fn dead_letter(&self, job: &ExtractionJob, reason: &str) -> Result<(), QueueError>;
}
