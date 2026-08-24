use sqlx::{PgPool, Postgres, Row, Transaction};
use struxio_common::models::BatchJob;
use struxio_common::WorkspaceId;
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

    pub async fn create_with_extractions(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        template_id: Uuid,
        document_ids: &[Uuid],
    ) -> Result<BatchJob, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let row = sqlx::query(&format!(
            "INSERT INTO batch_jobs (workspace_id, template_id, total_documents) \
             VALUES ($1, $2, $3) \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(template_id)
        .bind(i32::try_from(document_ids.len()).unwrap_or(i32::MAX))
        .fetch_one(&mut *tx)
        .await?;
        let batch = row_to_batch(row)?;

        for document_id in document_ids {
            let extraction = super::extractions::ExtractionRepo::create_in_tx(
                &mut tx,
                workspace_id,
                *document_id,
                template_id,
                Some(batch.id),
            )
            .await?;
            super::outbox::ExtractionOutboxRepo::insert_in_tx(&mut tx, &extraction).await?;
        }

        tx.commit().await?;
        Ok(batch)
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

    pub async fn recompute_progress(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<BatchJob, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let batch = Self::recompute_in_tx(&mut tx, workspace_id, id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        tx.commit().await?;
        Ok(batch)
    }

    pub async fn mark_processing(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE batch_jobs SET status = 'processing' \
             WHERE workspace_id = $1 AND id = $2 AND status = 'pending'",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn recompute_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<BatchJob>, sqlx::Error> {
        let locked = sqlx::query(
            "SELECT total_documents FROM batch_jobs WHERE workspace_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
        let Some(locked) = locked else {
            return Ok(None);
        };
        let total_documents: i32 = locked.get("total_documents");

        let counts = super::extractions::child_counts_in_tx(tx, workspace_id, id).await?;
        let status = counts.as_status_str(total_documents);
        let completed_at_now = counts.is_terminal(total_documents);

        let row = sqlx::query(&format!(
            r#"UPDATE batch_jobs SET
                completed_documents = $3,
                failed_documents = $4,
                status = $5,
                completed_at = CASE
                    WHEN $6 THEN COALESCE(completed_at, now())
                    ELSE NULL
                END
             WHERE workspace_id = $1 AND id = $2
             RETURNING {SELECT_COLS}"#,
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(counts.completed)
        .bind(counts.failed)
        .bind(status)
        .bind(completed_at_now)
        .fetch_one(&mut **tx)
        .await?;

        row_to_batch(row).map(Some)
    }
}
