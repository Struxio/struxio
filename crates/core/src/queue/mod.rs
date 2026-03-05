use async_trait::async_trait;
use uuid::Uuid;

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
    /// Opaque message identifier (e.g. Redis stream ID, Kafka offset string).
    /// Used only for acknowledgement — callers should treat it as opaque.
    pub stream_id: String,
    pub extraction_id: Uuid,
    pub document_id: Uuid,
    pub template_id: Uuid,
    pub org_id: Uuid,
    pub batch_job_id: Option<Uuid>,
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
        org_id: Uuid,
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
    /// Acknowledge successful processing of a job.
    async fn ack(&self, stream_id: &str) -> Result<(), QueueError>;
}
