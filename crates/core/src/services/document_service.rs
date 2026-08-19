use sqlx::PgPool;
use struxio_common::models::{
    CheckDocumentRequest, CheckDocumentResponse, ConfirmUploadRequest, Document,
};
use struxio_common::{mime::normalize_mime_type, AppError, PrincipalContext};
use struxio_db::repositories::documents::DocumentRepo;
use uuid::Uuid;

use crate::storage::StorageClient;

#[derive(Clone)]
pub struct DocumentService {
    db: PgPool,
    storage: StorageClient,
}

impl DocumentService {
    pub fn new(db: PgPool, storage: StorageClient) -> Self {
        Self { db, storage }
    }

    pub async fn check_document(
        &self,
        ctx: &PrincipalContext,
        request: &CheckDocumentRequest,
    ) -> Result<CheckDocumentResponse, AppError> {
        let mime_type = normalize_mime_type(&request.file_type)
            .map_err(|e| AppError::Validation(e.to_string()))?;

        if let Some(doc) =
            DocumentRepo::find_by_hash(&self.db, ctx.workspace_id(), &request.md5_hash)
                .await
                .map_err(|e| AppError::Database(e.to_string()))?
        {
            return Ok(CheckDocumentResponse {
                exists: true,
                document: Some(doc),
                upload_url: None,
                s3_key: None,
            });
        }

        let s3_key = format!(
            "{}/{}/{}",
            ctx.workspace_id().as_uuid(),
            Uuid::new_v4(),
            request.file_name
        );
        let upload_url = self
            .storage
            .generate_presigned_upload_url(&s3_key, mime_type, 3600)
            .await
            .map_err(|e| AppError::ExternalService(e.to_string()))?;

        Ok(CheckDocumentResponse {
            exists: false,
            document: None,
            upload_url: Some(upload_url),
            s3_key: Some(s3_key),
        })
    }

    pub async fn confirm_upload(
        &self,
        ctx: &PrincipalContext,
        request: &ConfirmUploadRequest,
    ) -> Result<Document, AppError> {
        let mime_type = normalize_mime_type(&request.file_type)
            .map_err(|e| AppError::Validation(e.to_string()))?;

        DocumentRepo::create(
            &self.db,
            ctx.workspace_id(),
            &request.md5_hash,
            &request.file_name,
            mime_type,
            &request.s3_key,
            request.size_bytes,
            1,
        )
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn list(&self, ctx: &PrincipalContext) -> Result<Vec<Document>, AppError> {
        DocumentRepo::list_all(&self.db, ctx.workspace_id())
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn get_by_id(
        &self,
        ctx: &PrincipalContext,
        document_id: Uuid,
    ) -> Result<Document, AppError> {
        DocumentRepo::find_by_id(&self.db, ctx.workspace_id(), document_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Document not found".to_string()))
    }

    pub async fn delete(&self, ctx: &PrincipalContext, document_id: Uuid) -> Result<(), AppError> {
        let s3_key = DocumentRepo::delete_by_id(&self.db, ctx.workspace_id(), document_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Document not found".to_string()))?;

        if let Err(e) = self.storage.delete(&s3_key).await {
            tracing::warn!(%e, s3_key, "Failed to delete document from S3; DB record already removed");
        }

        Ok(())
    }
}
