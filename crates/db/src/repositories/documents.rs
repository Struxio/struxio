use struxio_common::models::Document;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct DocumentRepo;

impl DocumentRepo {
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Document>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, md5_hash, file_name, file_type, s3_key, size_bytes, page_count, created_at \
             FROM documents WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| Document {
            id: r.get("id"),
            md5_hash: r.get("md5_hash"),
            file_name: r.get("file_name"),
            file_type: r.get("file_type"),
            s3_key: r.get("s3_key"),
            size_bytes: r.get("size_bytes"),
            page_count: r.get("page_count"),
            created_at: r.get("created_at"),
        }))
    }

    pub async fn find_by_hash(
        pool: &PgPool,
        md5_hash: &str,
    ) -> Result<Option<Document>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, md5_hash, file_name, file_type, s3_key, size_bytes, page_count, created_at \
             FROM documents WHERE md5_hash = $1",
        )
        .bind(md5_hash)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| Document {
            id: r.get("id"),
            md5_hash: r.get("md5_hash"),
            file_name: r.get("file_name"),
            file_type: r.get("file_type"),
            s3_key: r.get("s3_key"),
            size_bytes: r.get("size_bytes"),
            page_count: r.get("page_count"),
            created_at: r.get("created_at"),
        }))
    }

    pub async fn create(
        pool: &PgPool,
        md5_hash: &str,
        file_name: &str,
        file_type: &str,
        s3_key: &str,
        size_bytes: i64,
        page_count: i32,
    ) -> Result<Document, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO documents (md5_hash, file_name, file_type, s3_key, size_bytes, page_count) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING id, md5_hash, file_name, file_type, s3_key, size_bytes, page_count, created_at",
        )
        .bind(md5_hash)
        .bind(file_name)
        .bind(file_type)
        .bind(s3_key)
        .bind(size_bytes)
        .bind(page_count)
        .fetch_one(pool)
        .await?;

        Ok(Document {
            id: row.get("id"),
            md5_hash: row.get("md5_hash"),
            file_name: row.get("file_name"),
            file_type: row.get("file_type"),
            s3_key: row.get("s3_key"),
            size_bytes: row.get("size_bytes"),
            page_count: row.get("page_count"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn list_all(pool: &PgPool) -> Result<Vec<Document>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, md5_hash, file_name, file_type, s3_key, size_bytes, page_count, created_at \
             FROM documents ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Document {
                id: r.get("id"),
                md5_hash: r.get("md5_hash"),
                file_name: r.get("file_name"),
                file_type: r.get("file_type"),
                s3_key: r.get("s3_key"),
                size_bytes: r.get("size_bytes"),
                page_count: r.get("page_count"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    /// Delete a document by ID. Returns the s3_key so the caller can also clean up S3.
    pub async fn delete_by_id(
        pool: &PgPool,
        id: Uuid,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query("DELETE FROM documents WHERE id = $1 RETURNING s3_key")
            .bind(id)
            .fetch_optional(pool)
            .await?;

        Ok(row.map(|r| r.get("s3_key")))
    }
}
