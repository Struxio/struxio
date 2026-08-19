use async_trait::async_trait;
use struxio_common::WorkspaceId;
use uuid::Uuid;

use crate::jobs::JobEnvelope;

pub mod redis;

// ── Shared error type ───────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("redis error: {0}")]
    Redis(#[from] ::redis::RedisError),
    #[error("queue error: {0}")]
    Other(String),
}

// ── Shared job type ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExtractionJob {
    /// Opaque message identifier (e.g. Redis stream ID).
    /// Used only for acknowledgement — callers should treat it as opaque.
    pub stream_id: String,
    pub extraction_id: Uuid,
    pub document_id: Uuid,
    pub template_id: Uuid,
    pub workspace_id: WorkspaceId,
    pub batch_job_id: Option<Uuid>,
}

impl ExtractionJob {
    pub fn envelope(&self) -> JobEnvelope {
        JobEnvelope {
            extraction_id: self.extraction_id,
            document_id: self.document_id,
            template_id: self.template_id,
            workspace_id: self.workspace_id.as_uuid(),
            batch_job_id: self.batch_job_id,
        }
    }
}

/// A stream entry that could not be parsed into an [`ExtractionJob`].
#[derive(Debug, Clone)]
pub struct MalformedMessage {
    pub stream_id: String,
    pub reason: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum StreamDelivery {
    Job(ExtractionJob),
    Malformed(MalformedMessage),
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
/// Redis-specific reclaim, delayed retry, and DLQ live on
/// [`redis::RedisConsumer`] so this trait does not grow a broker abstraction.
#[async_trait]
pub trait QueueConsumer: Send + Sync + 'static {
    /// Ensure the consumer group / subscription exists (idempotent).
    async fn ensure_group(&self) -> Result<(), QueueError>;
    /// Poll for the next available job. Returns `None` on timeout with no job.
    async fn next_job(&self) -> Result<Option<ExtractionJob>, QueueError>;
    /// Acknowledge successful processing of a job.
    async fn ack(&self, stream_id: &str) -> Result<(), QueueError>;
}
