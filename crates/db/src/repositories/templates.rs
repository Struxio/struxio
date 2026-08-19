use struxio_common::models::ExtractionTemplate;
use struxio_common::WorkspaceId;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::workspace_id_of;

pub struct TemplateRepo;

fn row_to_template(r: sqlx::postgres::PgRow) -> Result<ExtractionTemplate, sqlx::Error> {
    Ok(ExtractionTemplate {
        id: r.get("id"),
        workspace_id: workspace_id_of(&r)?,
        name: r.get("name"),
        description: r.get("description"),
        json_schema: r.get("json_schema"),
        prompt_template: r.get("prompt_template"),
        is_system: r.get("is_system"),
        created_at: r.get("created_at"),
    })
}

const SELECT_COLS: &str =
    "id, workspace_id, name, description, json_schema, prompt_template, is_system, created_at";

impl TemplateRepo {
    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<ExtractionTemplate>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extraction_templates WHERE workspace_id = $1 AND id = $2",
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_template).transpose()
    }

    pub async fn create(
        pool: &PgPool,
        workspace_id: WorkspaceId,
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

        let row = sqlx::query(&format!(
            "INSERT INTO extraction_templates (workspace_id, name, description, json_schema, prompt_template, is_system) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING {SELECT_COLS}",
        ))
        .bind(workspace_id.as_uuid())
        .bind(name)
        .bind(description)
        .bind(json_value)
        .bind(prompt_template)
        .bind(is_system)
        .fetch_one(pool)
        .await?;

        row_to_template(row)
    }

    pub async fn list_all(
        pool: &PgPool,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ExtractionTemplate>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extraction_templates \
             WHERE workspace_id = $1 ORDER BY is_system DESC, name",
        ))
        .bind(workspace_id.as_uuid())
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(row_to_template).collect()
    }

    pub async fn update(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        json_schema: Option<&Value>,
        prompt_template: Option<&str>,
    ) -> Result<ExtractionTemplate, sqlx::Error> {
        let row = sqlx::query(&format!(
            r#"UPDATE extraction_templates SET
                name = COALESCE($3, name),
                description = COALESCE($4, description),
                json_schema = COALESCE($5, json_schema),
                prompt_template = COALESCE($6, prompt_template)
             WHERE workspace_id = $1 AND id = $2 AND is_system = false
             RETURNING {SELECT_COLS}"#,
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(json_schema.cloned())
        .bind(prompt_template)
        .fetch_one(pool)
        .await?;

        row_to_template(row)
    }

    pub async fn delete(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM extraction_templates WHERE workspace_id = $1 AND id = $2 AND is_system = false",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}
