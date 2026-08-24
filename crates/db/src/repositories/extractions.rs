use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::time::Duration;
use struxio_common::models::{ChildCounts, Extraction};
use struxio_common::WorkspaceId;
use uuid::Uuid;

use super::workspace_id_of;

pub struct ExtractionRepo;

fn row_to_extraction(r: sqlx::postgres::PgRow) -> Result<Extraction, sqlx::Error> {
    Ok(Extraction {
        id: r.get("id"),
        workspace_id: workspace_id_of(&r)?,
        document_id: r.get("document_id"),
        template_id: r.get("template_id"),
        batch_job_id: r.get("batch_job_id"),
        status: r.get("status"),
        result: r.get("result"),
        error_message: r.get("error_message"),
        model_id: r.get("model_id"),
        credits_charged: r.get("credits_charged"),
        input_tokens: r.get("input_tokens"),
        output_tokens: r.get("output_tokens"),
        processing_time_ms: r.get("processing_time_ms"),
        attempt: r.get("attempt"),
        created_at: r.get("created_at"),
        completed_at: r.get("completed_at"),
    })
}

const SELECT_COLS: &str =
    "id, workspace_id, document_id, template_id, batch_job_id, status, result, error_message, \
     model_id, credits_charged, input_tokens, output_tokens, processing_time_ms, attempt, created_at, completed_at";

impl ExtractionRepo {
    pub async fn create(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        document_id: Uuid,
        template_id: Uuid,
        batch_job_id: Option<Uuid>,
    ) -> Result<Extraction, sqlx::Error> {
        Self::create_with_status(
            pool,
            workspace_id,
            document_id,
            template_id,
            batch_job_id,
            "pending",
        )
        .await
    }

    pub async fn create_with_status(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        document_id: Uuid,
        template_id: Uuid,
        batch_job_id: Option<Uuid>,
        status: &str,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "INSERT INTO extractions (workspace_id, document_id, template_id, batch_job_id, status) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(document_id)
        .bind(template_id)
        .bind(batch_job_id)
        .bind(status)
        .fetch_one(pool)
        .await?;

        row_to_extraction(row)
    }

    pub async fn create_with_processing_lease(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        document_id: Uuid,
        template_id: Uuid,
        batch_job_id: Option<Uuid>,
        lease_token: Uuid,
        lease_duration: Duration,
    ) -> Result<Extraction, sqlx::Error> {
        let lease_millis = duration_millis(lease_duration);
        let row = sqlx::query(&format!(
            "INSERT INTO extractions \
             (workspace_id, document_id, template_id, batch_job_id, status, \
              processing_lease_token, processing_lease_expires_at) \
             VALUES ($1, $2, $3, $4, 'processing', $5, \
                     now() + ($6 * interval '1 millisecond')) \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(document_id)
        .bind(template_id)
        .bind(batch_job_id)
        .bind(lease_token)
        .bind(lease_millis)
        .fetch_one(pool)
        .await?;
        row_to_extraction(row)
    }

    pub(crate) async fn create_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        workspace_id: WorkspaceId,
        document_id: Uuid,
        template_id: Uuid,
        batch_job_id: Option<Uuid>,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "INSERT INTO extractions \
             (workspace_id, document_id, template_id, batch_job_id, status) \
             VALUES ($1, $2, $3, $4, 'pending') \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(document_id)
        .bind(template_id)
        .bind(batch_job_id)
        .fetch_one(&mut **tx)
        .await?;
        row_to_extraction(row)
    }

    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<Extraction>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extractions WHERE workspace_id = $1 AND id = $2",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_extraction).transpose()
    }

    pub async fn list_all(
        pool: &PgPool,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<Extraction>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extractions WHERE workspace_id = $1 ORDER BY created_at DESC",
        ))
        .bind(workspace_id.as_uuid())
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(row_to_extraction).collect()
    }

    pub async fn list_by_batch(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        batch_job_id: Uuid,
    ) -> Result<Vec<Extraction>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extractions \
             WHERE workspace_id = $1 AND batch_job_id = $2 ORDER BY created_at ASC",
        ))
        .bind(workspace_id.as_uuid())
        .bind(batch_job_id)
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(row_to_extraction).collect()
    }

    pub async fn count_by_batch_status(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        batch_job_id: Uuid,
    ) -> Result<(i32, i32), sqlx::Error> {
        let row = sqlx::query(
            "SELECT \
                COALESCE(COUNT(*) FILTER (WHERE status = 'completed'), 0)::int as completed, \
                COALESCE(COUNT(*) FILTER (WHERE status = 'failed'), 0)::int as failed \
             FROM extractions WHERE workspace_id = $1 AND batch_job_id = $2",
        )
        .bind(workspace_id.as_uuid())
        .bind(batch_job_id)
        .fetch_one(pool)
        .await?;

        Ok((row.get("completed"), row.get("failed")))
    }

    pub async fn update_status(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        status: &str,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET status = $3 WHERE workspace_id = $1 AND id = $2 RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(status)
        .fetch_one(pool)
        .await?;

        row_to_extraction(row)
    }

    pub async fn update_result(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        result: &Value,
        input_tokens: i32,
        output_tokens: i32,
        processing_time_ms: Option<i32>,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET result = $3, input_tokens = $4, output_tokens = $5, \
                    processing_time_ms = $6, status = 'completed', completed_at = now() \
             WHERE workspace_id = $1 AND id = $2 RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(result)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(processing_time_ms)
        .fetch_one(pool)
        .await?;

        row_to_extraction(row)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_completed(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        result: &Value,
        input_tokens: i32,
        output_tokens: i32,
        processing_time_ms: i32,
        model_id: &str,
        credits_charged: i32,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET status = 'completed', result = $3, input_tokens = $4, \
                    output_tokens = $5, processing_time_ms = $6, completed_at = now(), \
                    model_id = $7, credits_charged = $8 \
             WHERE workspace_id = $1 AND id = $2 RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(result)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(processing_time_ms)
        .bind(model_id)
        .bind(credits_charged)
        .fetch_one(pool)
        .await?;

        row_to_extraction(row)
    }

    pub async fn update_failed(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        error_message: &str,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET status = 'failed', error_message = $3, completed_at = now() \
             WHERE workspace_id = $1 AND id = $2 RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(error_message)
        .fetch_one(pool)
        .await?;

        row_to_extraction(row)
    }

    /// Exclusively claim an open extraction for execution.
    ///
    /// A live `processing` lease cannot be claimed by another worker. An expired
    /// lease may be reclaimed after a worker crash without incrementing `attempt`.
    pub async fn claim_for_processing(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        lease_token: Uuid,
        lease_duration: Duration,
    ) -> Result<ClaimOutcome, sqlx::Error> {
        let lease_millis = duration_millis(lease_duration);
        let row = sqlx::query(&format!(
            "UPDATE extractions SET \
                status = 'processing', \
                attempt = CASE \
                    WHEN status IN ('pending', 'retrying') THEN attempt + 1 \
                    ELSE attempt \
                END, \
                processing_lease_expires_at = now() + ($3 * interval '1 millisecond'), \
                processing_lease_token = $4 \
             WHERE workspace_id = $1 AND id = $2 \
               AND ( \
                    status IN ('pending', 'retrying') \
                    OR (status = 'processing' AND processing_lease_expires_at <= now()) \
               ) \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(lease_millis)
        .bind(lease_token)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            let extraction = row_to_extraction(row)?;
            if let Some(batch_id) = extraction.batch_job_id {
                super::batch_jobs::BatchJobRepo::mark_processing(pool, workspace_id, batch_id)
                    .await?;
            }
            return Ok(ClaimOutcome::Run(extraction));
        }

        match Self::find_by_id(pool, workspace_id, id).await? {
            Some(existing) if existing.status == "processing" => {
                Ok(ClaimOutcome::AlreadyProcessing(existing))
            }
            Some(existing) => Ok(ClaimOutcome::SkipTerminal(existing)),
            None => Ok(ClaimOutcome::Missing),
        }
    }

    pub async fn renew_processing_lease(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        lease_token: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE extractions \
             SET processing_lease_expires_at = now() + ($4 * interval '1 millisecond') \
             WHERE workspace_id = $1 AND id = $2 \
               AND status = 'processing' AND processing_lease_token = $3",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(lease_token)
        .bind(duration_millis(lease_duration))
        .execute(pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mark_retrying(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        error_message: &str,
    ) -> Result<Option<Extraction>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET status = 'retrying', error_message = $3, \
                    processing_lease_expires_at = NULL, processing_lease_token = NULL \
             WHERE workspace_id = $1 AND id = $2 \
               AND status NOT IN ('completed', 'failed') \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(error_message)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_extraction).transpose()
    }

    pub async fn mark_retrying_claimed(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        lease_token: Uuid,
        error_message: &str,
    ) -> Result<Option<Extraction>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET status = 'retrying', error_message = $4, \
                    processing_lease_expires_at = NULL, processing_lease_token = NULL \
             WHERE workspace_id = $1 AND id = $2 \
               AND status = 'processing' AND processing_lease_token = $3 \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(lease_token)
        .bind(error_message)
        .fetch_optional(pool)
        .await?;
        row.map(row_to_extraction).transpose()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn apply_completed(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        result: &Value,
        input_tokens: i32,
        output_tokens: i32,
        processing_time_ms: i32,
        model_id: &str,
    ) -> Result<ApplyTerminal, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let applied = apply_terminal_in_tx(
            &mut tx,
            workspace_id,
            id,
            None,
            TerminalWrite::Completed {
                result,
                input_tokens,
                output_tokens,
                processing_time_ms,
                model_id,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(applied)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn apply_completed_claimed(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        lease_token: Uuid,
        result: &Value,
        input_tokens: i32,
        output_tokens: i32,
        processing_time_ms: i32,
        model_id: &str,
    ) -> Result<ApplyTerminal, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let applied = apply_terminal_in_tx(
            &mut tx,
            workspace_id,
            id,
            Some(lease_token),
            TerminalWrite::Completed {
                result,
                input_tokens,
                output_tokens,
                processing_time_ms,
                model_id,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(applied)
    }

    pub async fn apply_failed(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        error_message: &str,
    ) -> Result<ApplyTerminal, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let applied = apply_terminal_in_tx(
            &mut tx,
            workspace_id,
            id,
            None,
            TerminalWrite::Failed { error_message },
        )
        .await?;
        tx.commit().await?;
        Ok(applied)
    }

    pub async fn apply_failed_claimed(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        lease_token: Uuid,
        error_message: &str,
    ) -> Result<ApplyTerminal, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let applied = apply_terminal_in_tx(
            &mut tx,
            workspace_id,
            id,
            Some(lease_token),
            TerminalWrite::Failed { error_message },
        )
        .await?;
        tx.commit().await?;
        Ok(applied)
    }
}

#[derive(Debug, Clone)]
pub enum ClaimOutcome {
    Run(Extraction),
    AlreadyProcessing(Extraction),
    SkipTerminal(Extraction),
    Missing,
}

#[derive(Debug, Clone)]
pub struct ApplyTerminal {
    pub extraction: Extraction,
    /// False when the row was already terminal; batch counters were not touched.
    pub changed: bool,
    pub batch: Option<struxio_common::models::BatchJob>,
}

enum TerminalWrite<'a> {
    Completed {
        result: &'a Value,
        input_tokens: i32,
        output_tokens: i32,
        processing_time_ms: i32,
        model_id: &'a str,
    },
    Failed {
        error_message: &'a str,
    },
}

async fn apply_terminal_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: WorkspaceId,
    id: Uuid,
    expected_lease_token: Option<Uuid>,
    write: TerminalWrite<'_>,
) -> Result<ApplyTerminal, sqlx::Error> {
    let row = match write {
        TerminalWrite::Completed {
            result,
            input_tokens,
            output_tokens,
            processing_time_ms,
            model_id,
        } => {
            sqlx::query(&format!(
                "UPDATE extractions SET status = 'completed', result = $3, input_tokens = $4, \
                        output_tokens = $5, processing_time_ms = $6, completed_at = now(), \
                        model_id = $7, credits_charged = 0, error_message = NULL, \
                        processing_lease_expires_at = NULL, processing_lease_token = NULL \
                 WHERE workspace_id = $1 AND id = $2 \
                   AND status NOT IN ('completed', 'failed') \
                   AND ($8::uuid IS NULL OR processing_lease_token = $8) \
                 RETURNING {SELECT_COLS}",
            ))
            .bind(workspace_id.as_uuid())
            .bind(id)
            .bind(result)
            .bind(input_tokens)
            .bind(output_tokens)
            .bind(processing_time_ms)
            .bind(model_id)
            .bind(expected_lease_token)
            .fetch_optional(&mut **tx)
            .await?
        }
        TerminalWrite::Failed { error_message } => {
            sqlx::query(&format!(
            "UPDATE extractions SET status = 'failed', error_message = $3, completed_at = now(), \
                    processing_lease_expires_at = NULL, processing_lease_token = NULL \
                 WHERE workspace_id = $1 AND id = $2 \
                   AND status NOT IN ('completed', 'failed') \
                   AND ($4::uuid IS NULL OR processing_lease_token = $4) \
                 RETURNING {SELECT_COLS}",
        ))
            .bind(workspace_id.as_uuid())
            .bind(id)
            .bind(error_message)
            .bind(expected_lease_token)
            .fetch_optional(&mut **tx)
            .await?
        }
    };

    let (extraction, changed) = match row {
        Some(row) => (row_to_extraction(row)?, true),
        None => {
            let existing = sqlx::query(&format!(
                "SELECT {SELECT_COLS} FROM extractions WHERE workspace_id = $1 AND id = $2",
            ))
            .bind(workspace_id.as_uuid())
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| sqlx::Error::RowNotFound)?;
            (row_to_extraction(existing)?, false)
        }
    };

    let batch = if changed {
        if let Some(batch_id) = extraction.batch_job_id {
            super::batch_jobs::BatchJobRepo::recompute_in_tx(tx, workspace_id, batch_id).await?
        } else {
            None
        }
    } else {
        None
    };

    Ok(ApplyTerminal {
        extraction,
        changed,
        batch,
    })
}

fn duration_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

pub(crate) async fn child_counts_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: WorkspaceId,
    batch_job_id: Uuid,
) -> Result<ChildCounts, sqlx::Error> {
    let row = sqlx::query(
        "SELECT \
            COALESCE(COUNT(*) FILTER (WHERE status = 'pending'), 0)::int AS pending, \
            COALESCE(COUNT(*) FILTER (WHERE status = 'processing'), 0)::int AS processing, \
            COALESCE(COUNT(*) FILTER (WHERE status = 'retrying'), 0)::int AS retrying, \
            COALESCE(COUNT(*) FILTER (WHERE status = 'completed'), 0)::int AS completed, \
            COALESCE(COUNT(*) FILTER (WHERE status = 'failed'), 0)::int AS failed \
         FROM extractions WHERE workspace_id = $1 AND batch_job_id = $2",
    )
    .bind(workspace_id.as_uuid())
    .bind(batch_job_id)
    .fetch_one(&mut **tx)
    .await?;

    Ok(ChildCounts {
        pending: row.get("pending"),
        processing: row.get("processing"),
        retrying: row.get("retrying"),
        completed: row.get("completed"),
        failed: row.get("failed"),
    })
}
