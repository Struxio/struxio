use sqlx::PgPool;
use struxio_common::models::{
    CheckDocumentRequest, CheckDocumentResponse, ConfirmUploadRequest, Document,
};
use struxio_common::{mime::normalize_mime_type, AppError, PrincipalContext, WorkspaceId};
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
        validate_workspace_s3_key(ctx.workspace_id(), &request.s3_key)?;

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

fn validate_workspace_s3_key(workspace_id: WorkspaceId, s3_key: &str) -> Result<(), AppError> {
    let prefix = format!("{}/", workspace_id.as_uuid());
    if s3_key.len() <= prefix.len() || !s3_key.starts_with(&prefix) {
        return Err(AppError::Validation(
            "s3_key does not belong to the authenticated workspace".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_workspace_s3_key;
    use struxio_common::WorkspaceId;
    use uuid::Uuid;

    #[test]
    fn upload_key_must_belong_to_workspace() {
        let owner = WorkspaceId::new(Uuid::new_v4()).unwrap();
        let other = WorkspaceId::new(Uuid::new_v4()).unwrap();
        let owned_key = format!("{}/upload/invoice.pdf", owner.as_uuid());
        let foreign_key = format!("{}/upload/invoice.pdf", other.as_uuid());

        assert!(validate_workspace_s3_key(owner, &owned_key).is_ok());
        assert!(validate_workspace_s3_key(owner, &foreign_key).is_err());
        assert!(validate_workspace_s3_key(owner, &format!("{}/", owner.as_uuid())).is_err());
    }
}
