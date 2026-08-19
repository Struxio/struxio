use struxio_common::models::BatchJob;
use struxio_common::WorkspaceId;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::workspace_id_of;

pub struct BatchJobRepo;

fn row_to_batch(r: sqlx::postgres::PgRow) -> Result<BatchJob, sqlx::Error> {
    Ok(BatchJob {
        id: r.get("id"),
        workspace_id: workspace_id_of(&r)?,
        template_id: r.get("template_id"),
        status: r.get("status"),
        total_documents: r.get("total_documents"),
        completed_documents: r.get("completed_documents"),
        failed_documents: r.get("failed_documents"),
        created_at: r.get("created_at"),
        completed_at: r.get("completed_at"),
    })
}

const SELECT_COLS: &str =
    "id, workspace_id, template_id, status, total_documents, completed_documents, failed_documents, created_at, completed_at";

impl BatchJobRepo {
    pub async fn create(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        template_id: Uuid,
        total_documents: i32,
    ) -> Result<BatchJob, sqlx::Error> {
        let row = sqlx::query(&format!(
            "INSERT INTO batch_jobs (workspace_id, template_id, total_documents) \
             VALUES ($1, $2, $3) \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(template_id)
        .bind(total_documents)
        .fetch_one(pool)
        .await?;

        row_to_batch(row)
    }

    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<BatchJob>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM batch_jobs WHERE workspace_id = $1 AND id = $2",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_batch).transpose()
    }

    pub async fn list_all(
        pool: &PgPool,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<BatchJob>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM batch_jobs WHERE workspace_id = $1 ORDER BY created_at DESC",
        ))
        .bind(workspace_id.as_uuid())
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(row_to_batch).collect()
    }

    pub async fn update_progress(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        completed_documents: i32,
        failed_documents: i32,
        status: &str,
    ) -> Result<BatchJob, sqlx::Error> {
        let row = sqlx::query(&format!(
            r#"UPDATE batch_jobs SET
                completed_documents = $3,
                failed_documents = $4,
                status = CASE
                    WHEN $3 + $4 >= total_documents THEN 'completed'
                    ELSE $5
                END,
                completed_at = CASE
                    WHEN $3 + $4 >= total_documents THEN now()
                    ELSE completed_at
                END
             WHERE workspace_id = $1 AND id = $2
             RETURNING {SELECT_COLS}"#,
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(completed_documents)
        .bind(failed_documents)
        .bind(status)
        .fetch_one(pool)
        .await?;

        row_to_batch(row)
    }
}
