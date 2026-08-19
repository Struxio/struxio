use serde_json::Value;
use sqlx::{PgPool, Row};
use struxio_common::models::Extraction;
use struxio_common::WorkspaceId;
use struxio_contracts::Sha256ContentHash;
use uuid::Uuid;

use super::wave2::content_hash_bytes;
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
        created_at: r.get("created_at"),
        completed_at: r.get("completed_at"),
    })
}

const SELECT_COLS: &str =
    "id, workspace_id, document_id, template_id, batch_job_id, status, result, error_message, \
     model_id, credits_charged, input_tokens, output_tokens, processing_time_ms, created_at, completed_at";

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

    /// Create an extraction tied to an immutable published contract version.
    /// The contract hash participates in the composite FK, so a contract ID
    /// cannot be reused across workspaces or silently repointed.
    pub async fn create_with_contract(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        document_id: Uuid,
        template_id: Uuid,
        batch_job_id: Option<Uuid>,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "INSERT INTO extractions \
             (workspace_id, document_id, template_id, batch_job_id, contract_id, \
              contract_content_sha256, status) \
             VALUES ($1, $2, $3, $4, $5, $6, 'pending') \
             RETURNING {SELECT_COLS}"
        ))
        .bind(workspace_id.as_uuid())
        .bind(document_id)
        .bind(template_id)
        .bind(batch_job_id)
        .bind(contract_id)
        .bind(content_hash_bytes(contract_content_sha256))
        .fetch_one(pool)
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
}
