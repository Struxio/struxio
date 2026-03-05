use struxio_common::models::{BatchJob, CreateBatchRequest, Extraction};
use struxio_common::AppError;
use struxio_db::repositories::{
    batch_jobs::BatchJobRepo,
    documents::DocumentRepo,
    extractions::ExtractionRepo,
    templates::TemplateRepo,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::queue::QueueProducer;

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
        request: &CreateBatchRequest,
        model_id: &str,
    ) -> Result<BatchJob, AppError> {
        TemplateRepo::find_by_id(&self.db, request.template_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))?;

        let total_documents = request.document_ids.len() as i32;
        if total_documents == 0 {
            return Err(AppError::Validation("document_ids must not be empty".to_string()));
        }

        let batch = BatchJobRepo::create(&self.db, request.template_id, total_documents)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        for doc_id in &request.document_ids {
            DocumentRepo::find_by_id(&self.db, *doc_id)
                .await
                .map_err(|e| AppError::Database(e.to_string()))?
                .ok_or_else(|| AppError::NotFound("Document not found".to_string()))?;

            let extraction = ExtractionRepo::create(
                &self.db,
                *doc_id,
                request.template_id,
                Some(batch.id),
            )
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

            self.queue
                .enqueue_extraction(
                    extraction.id,
                    extraction.document_id,
                    extraction.template_id,
                    Uuid::nil(),
                    extraction.batch_job_id,
                )
                .await
                .map_err(|e| AppError::ExternalService(e.to_string()))?;
        }

        let _ = model_id; // reserved for future per-model pricing
        Ok(batch)
    }

    pub async fn list(&self) -> Result<Vec<BatchJob>, AppError> {
        BatchJobRepo::list_all(&self.db)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn get(&self, batch_id: Uuid) -> Result<BatchJob, AppError> {
        BatchJobRepo::find_by_id(&self.db, batch_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Batch not found".to_string()))
    }

    pub async fn list_extractions(&self, batch_id: Uuid) -> Result<Vec<Extraction>, AppError> {
        // Verify batch exists first
        self.get(batch_id).await?;

        ExtractionRepo::list_by_batch(&self.db, batch_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }
}
