use struxio_common::models::ExtractionTemplate;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct TemplateRepo;

impl TemplateRepo {
    pub async fn find_by_id(
        pool: &PgPool,
        id: Uuid,
    ) -> Result<Option<ExtractionTemplate>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, description, json_schema, prompt_template, is_system, created_at \
             FROM extraction_templates WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| ExtractionTemplate {
            id: r.get("id"),
            name: r.get("name"),
            description: r.get("description"),
            json_schema: r.get("json_schema"),
            prompt_template: r.get("prompt_template"),
            is_system: r.get("is_system"),
            created_at: r.get("created_at"),
        }))
    }

    pub async fn create(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
        json_schema: &Value,
        prompt_template: &str,
        is_system: bool,
    ) -> Result<ExtractionTemplate, sqlx::Error> {
        let json_value = serde_json::to_value(json_schema).map_err(|e| {
            sqlx::Error::ColumnDecode {
                index: "json_schema".into(),
                source: Box::new(e),
            }
        })?;

        let row = sqlx::query(
            "INSERT INTO extraction_templates (name, description, json_schema, prompt_template, is_system) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING id, name, description, json_schema, prompt_template, is_system, created_at",
        )
        .bind(name)
        .bind(description)
        .bind(json_value)
        .bind(prompt_template)
        .bind(is_system)
        .fetch_one(pool)
        .await?;

        Ok(ExtractionTemplate {
            id: row.get("id"),
            name: row.get("name"),
            description: row.get("description"),
            json_schema: row.get("json_schema"),
            prompt_template: row.get("prompt_template"),
            is_system: row.get("is_system"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn list_all(pool: &PgPool) -> Result<Vec<ExtractionTemplate>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, name, description, json_schema, prompt_template, is_system, created_at \
             FROM extraction_templates ORDER BY is_system DESC, name",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ExtractionTemplate {
                id: r.get("id"),
                name: r.get("name"),
                description: r.get("description"),
                json_schema: r.get("json_schema"),
                prompt_template: r.get("prompt_template"),
                is_system: r.get("is_system"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        json_schema: Option<&Value>,
        prompt_template: Option<&str>,
    ) -> Result<ExtractionTemplate, sqlx::Error> {
        let row = sqlx::query(
            r#"UPDATE extraction_templates SET
                name = COALESCE($2, name),
                description = COALESCE($3, description),
                json_schema = COALESCE($4, json_schema),
                prompt_template = COALESCE($5, prompt_template)
             WHERE id = $1 AND is_system = false
             RETURNING id, name, description, json_schema, prompt_template, is_system, created_at"#,
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(json_schema.cloned())
        .bind(prompt_template)
        .fetch_one(pool)
        .await?;

        Ok(ExtractionTemplate {
            id: row.get("id"),
            name: row.get("name"),
            description: row.get("description"),
            json_schema: row.get("json_schema"),
            prompt_template: row.get("prompt_template"),
            is_system: row.get("is_system"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM extraction_templates WHERE id = $1 AND is_system = false",
        )
        .bind(id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}
