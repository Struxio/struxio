use struxio_common::models::Document;
use struxio_common::WorkspaceId;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::workspace_id_of;

pub struct DocumentRepo;

fn row_to_document(r: sqlx::postgres::PgRow) -> Result<Document, sqlx::Error> {
    Ok(Document {
        id: r.get("id"),
        workspace_id: workspace_id_of(&r)?,
        md5_hash: r.get("md5_hash"),
        file_name: r.get("file_name"),
        file_type: r.get("file_type"),
        s3_key: r.get("s3_key"),
        size_bytes: r.get("size_bytes"),
        page_count: r.get("page_count"),
        created_at: r.get("created_at"),
    })
}

const SELECT_COLS: &str =
    "id, workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes, page_count, created_at";

impl DocumentRepo {
    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<Document>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM documents WHERE workspace_id = $1 AND id = $2",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_document).transpose()
    }

    pub async fn find_by_hash(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        md5_hash: &str,
    ) -> Result<Option<Document>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM documents WHERE workspace_id = $1 AND md5_hash = $2",
        ))
        .bind(workspace_id.as_uuid())
        .bind(md5_hash)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_document).transpose()
    }

    pub async fn create(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        md5_hash: &str,
        file_name: &str,
        file_type: &str,
        s3_key: &str,
        size_bytes: i64,
        page_count: i32,
    ) -> Result<Document, sqlx::Error> {
        let row = sqlx::query(&format!(
            "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes, page_count) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(md5_hash)
        .bind(file_name)
        .bind(file_type)
        .bind(s3_key)
        .bind(size_bytes)
        .bind(page_count)
        .fetch_one(pool)
        .await?;

        row_to_document(row)
    }

    pub async fn list_all(
        pool: &PgPool,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<Document>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM documents WHERE workspace_id = $1 ORDER BY created_at DESC",
        ))
        .bind(workspace_id.as_uuid())
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(row_to_document).collect()
    }

    /// Delete a document by ID. Returns the s3_key so the caller can also clean up S3.
    pub async fn delete_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query(
            "DELETE FROM documents WHERE workspace_id = $1 AND id = $2 RETURNING s3_key",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| r.get("s3_key")))
    }
}
