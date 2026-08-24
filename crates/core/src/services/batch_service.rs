use sqlx::PgPool;
use struxio_common::models::{BatchJob, CreateBatchRequest, Extraction};
use struxio_common::{AppError, PrincipalContext};
use struxio_db::repositories::{
    batch_jobs::BatchJobRepo, documents::DocumentRepo, extractions::ExtractionRepo,
    templates::TemplateRepo,
};
use uuid::Uuid;

use crate::queue::QueueProducer;
use crate::services::outbox_service::flush_batch_outbox;

#[derive(Clone)]
pub struct BatchService<Q: QueueProducer> {
    db: PgPool,
    queue: Q,
}

impl<Q: QueueProducer> BatchService<Q> {
    pub fn new(db: PgPool, queue: Q) -> Self {
        Self { db, queue }
    }

    pub async fn create(
        &self,
        ctx: &PrincipalContext,
        request: &CreateBatchRequest,
        model_id: &str,
    ) -> Result<BatchJob, AppError> {
        TemplateRepo::find_by_id(&self.db, ctx.workspace_id(), request.template_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))?;

        if request.document_ids.is_empty() {
            return Err(AppError::Validation(
                "document_ids must not be empty".to_string(),
            ));
        }

        for doc_id in &request.document_ids {
            DocumentRepo::find_by_id(&self.db, ctx.workspace_id(), *doc_id)
                .await
                .map_err(|e| AppError::Database(e.to_string()))?
                .ok_or_else(|| AppError::NotFound("Document not found".to_string()))?;
        }

        let batch = BatchJobRepo::create_with_extractions(
            &self.db,
            ctx.workspace_id(),
            request.template_id,
            &request.document_ids,
        )
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        match flush_batch_outbox(&self.db, &self.queue, ctx.workspace_id(), batch.id).await {
            Ok(flush) if flush.failed > 0 => {
                tracing::warn!(
                    batch_id = %batch.id,
                    published = flush.published,
                    failed = flush.failed,
                    "Batch committed with pending outbox jobs"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    batch_id = %batch.id,
                    %error,
                    "Batch committed; outbox publishing will be retried"
                );
            }
        }

        let _ = model_id;
        Ok(batch)
    }

    pub async fn list(&self, ctx: &PrincipalContext) -> Result<Vec<BatchJob>, AppError> {
        BatchJobRepo::list_all(&self.db, ctx.workspace_id())
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn get(&self, ctx: &PrincipalContext, batch_id: Uuid) -> Result<BatchJob, AppError> {
        BatchJobRepo::find_by_id(&self.db, ctx.workspace_id(), batch_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Batch not found".to_string()))
    }

    pub async fn list_extractions(
        &self,
        ctx: &PrincipalContext,
        batch_id: Uuid,
    ) -> Result<Vec<Extraction>, AppError> {
        self.get(ctx, batch_id).await?;

        ExtractionRepo::list_by_batch(&self.db, ctx.workspace_id(), batch_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }
}
