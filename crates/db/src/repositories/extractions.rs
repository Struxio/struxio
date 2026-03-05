use struxio_common::models::Extraction;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct ExtractionRepo;

fn row_to_extraction(r: sqlx::postgres::PgRow) -> Extraction {
    Extraction {
        id: r.get("id"),
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
    }
}

const SELECT_COLS: &str =
    "id, document_id, template_id, batch_job_id, status, result, error_message, \
     model_id, credits_charged, input_tokens, output_tokens, processing_time_ms, created_at, completed_at";

impl ExtractionRepo {
    pub async fn create(
        pool: &PgPool,
        document_id: Uuid,
        template_id: Uuid,
        batch_job_id: Option<Uuid>,
    ) -> Result<Extraction, sqlx::Error> {
        Self::create_with_status(pool, document_id, template_id, batch_job_id, "pending").await
    }

    pub async fn create_with_status(
        pool: &PgPool,
        document_id: Uuid,
        template_id: Uuid,
        batch_job_id: Option<Uuid>,
        status: &str,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "INSERT INTO extractions (document_id, template_id, batch_job_id, status) \
             VALUES ($1, $2, $3, $4) \
             RETURNING {SELECT_COLS}",
        ))
        .bind(document_id)
        .bind(template_id)
        .bind(batch_job_id)
        .bind(status)
        .fetch_one(pool)
        .await?;

        Ok(row_to_extraction(row))
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Extraction>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extractions WHERE id = $1",
        ))
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(row_to_extraction))
    }

    pub async fn list_all(pool: &PgPool) -> Result<Vec<Extraction>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extractions ORDER BY created_at DESC",
        ))
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(row_to_extraction).collect())
    }

    pub async fn list_by_batch(
        pool: &PgPool,
        batch_job_id: Uuid,
    ) -> Result<Vec<Extraction>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extractions WHERE batch_job_id = $1 ORDER BY created_at ASC",
        ))
        .bind(batch_job_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(row_to_extraction).collect())
    }

    pub async fn count_by_batch_status(
        pool: &PgPool,
        batch_job_id: Uuid,
    ) -> Result<(i32, i32), sqlx::Error> {
        let row = sqlx::query(
            "SELECT \
                COALESCE(COUNT(*) FILTER (WHERE status = 'completed'), 0)::int as completed, \
                COALESCE(COUNT(*) FILTER (WHERE status = 'failed'), 0)::int as failed \
             FROM extractions WHERE batch_job_id = $1",
        )
        .bind(batch_job_id)
        .fetch_one(pool)
        .await?;

        Ok((row.get("completed"), row.get("failed")))
    }

    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: &str,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET status = $2 WHERE id = $1 RETURNING {SELECT_COLS}",
        ))
        .bind(id)
        .bind(status)
        .fetch_one(pool)
        .await?;

        Ok(row_to_extraction(row))
    }

    pub async fn update_result(
        pool: &PgPool,
        id: Uuid,
        result: &Value,
        input_tokens: i32,
        output_tokens: i32,
        processing_time_ms: Option<i32>,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET result = $2, input_tokens = $3, output_tokens = $4, \
                    processing_time_ms = $5, status = 'completed', completed_at = now() \
             WHERE id = $1 RETURNING {SELECT_COLS}",
        ))
        .bind(id)
        .bind(result)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(processing_time_ms)
        .fetch_one(pool)
        .await?;

        Ok(row_to_extraction(row))
    }

    pub async fn update_completed(
        pool: &PgPool,
        id: Uuid,
        result: &Value,
        input_tokens: i32,
        output_tokens: i32,
        processing_time_ms: i32,
        model_id: &str,
        credits_charged: i32,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET status = 'completed', result = $2, input_tokens = $3, \
                    output_tokens = $4, processing_time_ms = $5, completed_at = now(), \
                    model_id = $6, credits_charged = $7 \
             WHERE id = $1 RETURNING {SELECT_COLS}",
        ))
        .bind(id)
        .bind(result)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(processing_time_ms)
        .bind(model_id)
        .bind(credits_charged)
        .fetch_one(pool)
        .await?;

        Ok(row_to_extraction(row))
    }

    pub async fn update_failed(
        pool: &PgPool,
        id: Uuid,
        error_message: &str,
    ) -> Result<Extraction, sqlx::Error> {
        let row = sqlx::query(&format!(
            "UPDATE extractions SET status = 'failed', error_message = $2, completed_at = now() \
             WHERE id = $1 RETURNING {SELECT_COLS}",
        ))
        .bind(id)
        .bind(error_message)
        .fetch_one(pool)
        .await?;

        Ok(row_to_extraction(row))
    }
}
