use struxio_common::models::BatchJob;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct BatchJobRepo;

impl BatchJobRepo {
    pub async fn create(
        pool: &PgPool,
        template_id: Uuid,
        total_documents: i32,
    ) -> Result<BatchJob, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO batch_jobs (template_id, total_documents) \
             VALUES ($1, $2) \
             RETURNING id, template_id, status, total_documents, completed_documents, failed_documents, created_at, completed_at",
        )
        .bind(template_id)
        .bind(total_documents)
        .fetch_one(pool)
        .await?;

        Ok(BatchJob {
            id: row.get("id"),
            template_id: row.get("template_id"),
            status: row.get("status"),
            total_documents: row.get("total_documents"),
            completed_documents: row.get("completed_documents"),
            failed_documents: row.get("failed_documents"),
            created_at: row.get("created_at"),
            completed_at: row.get("completed_at"),
        })
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<BatchJob>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, template_id, status, total_documents, completed_documents, failed_documents, created_at, completed_at \
             FROM batch_jobs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| BatchJob {
            id: r.get("id"),
            template_id: r.get("template_id"),
            status: r.get("status"),
            total_documents: r.get("total_documents"),
            completed_documents: r.get("completed_documents"),
            failed_documents: r.get("failed_documents"),
            created_at: r.get("created_at"),
            completed_at: r.get("completed_at"),
        }))
    }

    pub async fn list_all(pool: &PgPool) -> Result<Vec<BatchJob>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, template_id, status, total_documents, completed_documents, failed_documents, created_at, completed_at \
             FROM batch_jobs ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| BatchJob {
                id: r.get("id"),
                template_id: r.get("template_id"),
                status: r.get("status"),
                total_documents: r.get("total_documents"),
                completed_documents: r.get("completed_documents"),
                failed_documents: r.get("failed_documents"),
                created_at: r.get("created_at"),
                completed_at: r.get("completed_at"),
            })
            .collect())
    }

    pub async fn update_progress(
        pool: &PgPool,
        id: Uuid,
        completed_documents: i32,
        failed_documents: i32,
        status: &str,
    ) -> Result<BatchJob, sqlx::Error> {
        let row = sqlx::query(
            r#"UPDATE batch_jobs SET
                completed_documents = $2,
                failed_documents = $3,
                status = CASE
                    WHEN $2 + $3 >= total_documents THEN 'completed'
                    ELSE $4
                END,
                completed_at = CASE
                    WHEN $2 + $3 >= total_documents THEN now()
                    ELSE completed_at
                END
             WHERE id = $1
             RETURNING id, template_id, status, total_documents, completed_documents, failed_documents, created_at, completed_at"#,
        )
        .bind(id)
        .bind(completed_documents)
        .bind(failed_documents)
        .bind(status)
        .fetch_one(pool)
        .await?;

        Ok(BatchJob {
            id: row.get("id"),
            template_id: row.get("template_id"),
            status: row.get("status"),
            total_documents: row.get("total_documents"),
            completed_documents: row.get("completed_documents"),
            failed_documents: row.get("failed_documents"),
            created_at: row.get("created_at"),
            completed_at: row.get("completed_at"),
        })
    }
}
