use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};
use struxio_common::{models::Extraction, WorkspaceId};
use uuid::Uuid;

use super::workspace_id_of;

#[derive(Debug, Clone)]
pub struct ExtractionOutboxEntry {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub extraction_id: Uuid,
    pub document_id: Uuid,
    pub template_id: Uuid,
    pub batch_job_id: Option<Uuid>,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub struct ExtractionOutboxRepo;

const SELECT_COLS: &str = "id, workspace_id, extraction_id, document_id, template_id, \
    batch_job_id, attempts, last_error, published_at, created_at";

fn row_to_entry(row: sqlx::postgres::PgRow) -> Result<ExtractionOutboxEntry, sqlx::Error> {
    Ok(ExtractionOutboxEntry {
        id: row.get("id"),
        workspace_id: workspace_id_of(&row)?,
        extraction_id: row.get("extraction_id"),
        document_id: row.get("document_id"),
        template_id: row.get("template_id"),
        batch_job_id: row.get("batch_job_id"),
        attempts: row.get("attempts"),
        last_error: row.get("last_error"),
        published_at: row.get("published_at"),
        created_at: row.get("created_at"),
    })
}

impl ExtractionOutboxRepo {
    pub(crate) async fn insert_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        extraction: &Extraction,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO extraction_outbox \
             (workspace_id, extraction_id, document_id, template_id, batch_job_id) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(extraction.workspace_id.as_uuid())
        .bind(extraction.id)
        .bind(extraction.document_id)
        .bind(extraction.template_id)
        .bind(extraction.batch_job_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn list_pending(
        pool: &PgPool,
        limit: usize,
    ) -> Result<Vec<ExtractionOutboxEntry>, sqlx::Error> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extraction_outbox \
             WHERE published_at IS NULL ORDER BY created_at, id LIMIT $1",
        ))
        .bind(limit)
        .fetch_all(pool)
        .await?;
        rows.into_iter().map(row_to_entry).collect()
    }

    pub async fn list_pending_for_batch(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        batch_job_id: Uuid,
    ) -> Result<Vec<ExtractionOutboxEntry>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extraction_outbox \
             WHERE workspace_id = $1 AND batch_job_id = $2 AND published_at IS NULL \
             ORDER BY created_at, id",
        ))
        .bind(workspace_id.as_uuid())
        .bind(batch_job_id)
        .fetch_all(pool)
        .await?;
        rows.into_iter().map(row_to_entry).collect()
    }

    pub async fn mark_published(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE extraction_outbox SET published_at = now(), last_error = NULL \
             WHERE id = $1 AND published_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn record_failure(pool: &PgPool, id: Uuid, error: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE extraction_outbox SET attempts = attempts + 1, last_error = $2 \
             WHERE id = $1 AND published_at IS NULL",
        )
        .bind(id)
        .bind(error)
        .execute(pool)
        .await?;
        Ok(())
    }
}
