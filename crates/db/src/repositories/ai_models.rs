use sqlx::{PgPool, Row};
use struxio_common::models::AiModel;

pub struct AiModelRepo;

impl AiModelRepo {
    pub async fn find_by_id(db: &PgPool, id: &str) -> Result<Option<AiModel>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, display_name, credit_cost_per_page, is_active, created_at
            FROM ai_models
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(db)
        .await?;

        Ok(row.map(|r| AiModel {
            id: r.get("id"),
            display_name: r.get("display_name"),
            credit_cost_per_page: r.get("credit_cost_per_page"),
            is_active: r.get("is_active"),
            created_at: r.get("created_at"),
        }))
    }

    pub async fn find_active(db: &PgPool) -> Result<Vec<AiModel>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, display_name, credit_cost_per_page, is_active, created_at
            FROM ai_models
            WHERE is_active = true
            ORDER BY id
            "#,
        )
        .fetch_all(db)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| AiModel {
                id: r.get("id"),
                display_name: r.get("display_name"),
                credit_cost_per_page: r.get("credit_cost_per_page"),
                is_active: r.get("is_active"),
                created_at: r.get("created_at"),
            })
            .collect())
    }
}
