use struxio_common::models::{
    CreateExtractionRequest, Extraction, ExtractionTemplate, InlineExtractionRequest,
};
use struxio_common::{mime::normalize_mime_type, AppError};
use struxio_db::repositories::{
    documents::DocumentRepo,
    extractions::ExtractionRepo,
    templates::TemplateRepo,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::gemini::GeminiClient;
use crate::queue::QueueProducer;
use crate::storage::StorageClient;

#[derive(Clone)]
pub struct ExtractionService<Q: QueueProducer> {
    db: PgPool,
    queue: Q,
    storage: StorageClient,
    gemini: GeminiClient,
}

impl<Q: QueueProducer> ExtractionService<Q> {
    pub fn new(
        db: PgPool,
        queue: Q,
        storage: StorageClient,
        gemini: GeminiClient,
    ) -> Self {
        Self { db, queue, storage, gemini }
    }

    pub async fn create(
        &self,
        request: &CreateExtractionRequest,
    ) -> Result<Extraction, AppError> {
        // Ensure document and template exist
        DocumentRepo::find_by_id(&self.db, request.document_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Document not found".to_string()))?;

        TemplateRepo::find_by_id(&self.db, request.template_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))?;

        let extraction = ExtractionRepo::create(
            &self.db,
            request.document_id,
            request.template_id,
            None,
        )
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.queue
            .enqueue_extraction(
                extraction.id,
                extraction.document_id,
                extraction.template_id,
                // org_id is not used in queue messages for OSS; pass Uuid::nil() as placeholder
                Uuid::nil(),
                extraction.batch_job_id,
            )
            .await
            .map_err(|e| AppError::ExternalService(e.to_string()))?;

        Ok(extraction)
    }

    pub async fn create_sync(
        &self,
        request: &CreateExtractionRequest,
        model_id: &str,
    ) -> Result<Extraction, AppError> {
        let doc = DocumentRepo::find_by_id(&self.db, request.document_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Document not found".to_string()))?;

        let template = TemplateRepo::find_by_id(&self.db, request.template_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))?;

        let mime_type = normalize_mime_type(&doc.file_type)
            .map_err(|e| AppError::Validation(e.to_string()))?;

        let extraction = ExtractionRepo::create_with_status(
            &self.db,
            request.document_id,
            request.template_id,
            None,
            "processing",
        )
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        let file_bytes = self
            .storage
            .download(&doc.s3_key)
            .await
            .map_err(|e| AppError::ExternalService(e.to_string()))?;

        let start = std::time::Instant::now();
        match self
            .gemini
            .extract(&file_bytes, mime_type, &template.prompt_template, &template.json_schema)
            .await
        {
            Ok(response) => {
                let processing_time_ms = start.elapsed().as_millis() as i32;
                ExtractionRepo::update_completed(
                    &self.db,
                    extraction.id,
                    &response.result,
                    response.input_tokens,
                    response.output_tokens,
                    processing_time_ms,
                    model_id,
                    0,
                )
                .await
                .map_err(|e| AppError::Database(e.to_string()))
            }
            Err(e) => {
                let err_msg = e.to_string();
                ExtractionRepo::update_failed(&self.db, extraction.id, &err_msg)
                    .await
                    .map_err(|db_err| AppError::Database(db_err.to_string()))
            }
        }
    }

    pub async fn create_inline(
        &self,
        request: &InlineExtractionRequest,
        model_id: &str,
    ) -> Result<Extraction, AppError> {
        use base64::{engine::general_purpose::STANDARD, Engine};

        let mime_type = normalize_mime_type(&request.file_type)
            .map_err(|e| AppError::Validation(e.to_string()))?;

        // 1. Decode base64
        let file_bytes = STANDARD
            .decode(&request.file_base64)
            .map_err(|e| AppError::Validation(format!("Invalid base64: {e}")))?;

        // 2. Compute MD5 for deduplication
        let md5_hash = format!("{:x}", md5::compute(&file_bytes));

        // 3. Reuse existing document or upload + create a new one
        let document = match DocumentRepo::find_by_hash(&self.db, &md5_hash)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
        {
            Some(doc) => doc,
            None => {
                let s3_key = format!("{}/{}", uuid::Uuid::new_v4(), request.file_name);
                self.storage
                    .upload(&s3_key, &file_bytes, mime_type)
                    .await
                    .map_err(|e| AppError::ExternalService(e.to_string()))?;

                DocumentRepo::create(
                    &self.db,
                    &md5_hash,
                    &request.file_name,
                    mime_type,
                    &s3_key,
                    file_bytes.len() as i64,
                    1,
                )
                .await
                .map_err(|e| AppError::Database(e.to_string()))?
            }
        };

        // 4. Run synchronous extraction against the document
        let inline_req = CreateExtractionRequest {
            document_id: document.id,
            template_id: request.template_id,
        };
        self.create_sync(&inline_req, model_id).await
    }

    pub async fn list(&self) -> Result<Vec<Extraction>, AppError> {
        ExtractionRepo::list_all(&self.db)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn get(&self, extraction_id: Uuid) -> Result<Extraction, AppError> {
        ExtractionRepo::find_by_id(&self.db, extraction_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Extraction not found".to_string()))
    }

    fn _can_access_template(template: &ExtractionTemplate) -> bool {
        // OSS: all templates are accessible — no org scoping
        let _ = template;
        true
    }
}
