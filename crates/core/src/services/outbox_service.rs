use sqlx::PgPool;
use struxio_common::AppError;
use struxio_common::WorkspaceId;
use struxio_db::repositories::outbox::{ExtractionOutboxEntry, ExtractionOutboxRepo};
use uuid::Uuid;

use crate::queue::QueueProducer;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutboxFlush {
    pub published: usize,
    pub failed: usize,
}

pub async fn flush_extraction_outbox<Q: QueueProducer>(
    db: &PgPool,
    queue: &Q,
    limit: usize,
) -> Result<OutboxFlush, AppError> {
    let entries = ExtractionOutboxRepo::list_pending(db, limit)
        .await
        .map_err(|error| AppError::Database(error.to_string()))?;
    publish_entries(db, queue, entries).await
}

pub async fn flush_batch_outbox<Q: QueueProducer>(
    db: &PgPool,
    queue: &Q,
    workspace_id: WorkspaceId,
    batch_job_id: Uuid,
) -> Result<OutboxFlush, AppError> {
    let entries = ExtractionOutboxRepo::list_pending_for_batch(db, workspace_id, batch_job_id)
        .await
        .map_err(|error| AppError::Database(error.to_string()))?;
    publish_entries(db, queue, entries).await
}

async fn publish_entries<Q: QueueProducer>(
    db: &PgPool,
    queue: &Q,
    entries: Vec<ExtractionOutboxEntry>,
) -> Result<OutboxFlush, AppError> {
    let mut result = OutboxFlush::default();

    for entry in entries {
        match queue
            .enqueue_extraction(
                entry.extraction_id,
                entry.document_id,
                entry.template_id,
                entry.workspace_id,
                entry.batch_job_id,
            )
            .await
        {
            Ok(()) => {
                ExtractionOutboxRepo::mark_published(db, entry.id)
                    .await
                    .map_err(|error| AppError::Database(error.to_string()))?;
                result.published += 1;
            }
            Err(error) => {
                ExtractionOutboxRepo::record_failure(db, entry.id, &error.to_string())
                    .await
                    .map_err(|db_error| AppError::Database(db_error.to_string()))?;
                result.failed += 1;
            }
        }
    }

    Ok(result)
}
