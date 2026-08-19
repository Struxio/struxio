use sqlx::PgPool;
use struxio_common::models::{CreateTemplateRequest, ExtractionTemplate, UpdateTemplateRequest};
use struxio_common::{AppError, PrincipalContext};
use struxio_db::repositories::templates::TemplateRepo;
use uuid::Uuid;

#[derive(Clone)]
pub struct TemplateService {
    db: PgPool,
}

impl TemplateService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn list(&self, ctx: &PrincipalContext) -> Result<Vec<ExtractionTemplate>, AppError> {
        TemplateRepo::list_all(&self.db, ctx.workspace_id())
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn get(
        &self,
        ctx: &PrincipalContext,
        template_id: Uuid,
    ) -> Result<ExtractionTemplate, AppError> {
        TemplateRepo::find_by_id(&self.db, ctx.workspace_id(), template_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))
    }

    pub async fn create(
        &self,
        ctx: &PrincipalContext,
        request: &CreateTemplateRequest,
    ) -> Result<ExtractionTemplate, AppError> {
        Self::validate_json_schema(&request.json_schema)?;

        TemplateRepo::create(
            &self.db,
            ctx.workspace_id(),
            &request.name,
            request.description.as_deref(),
            &request.json_schema,
            &request.prompt_template,
            false,
        )
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn update(
        &self,
        ctx: &PrincipalContext,
        template_id: Uuid,
        request: &UpdateTemplateRequest,
    ) -> Result<ExtractionTemplate, AppError> {
        Self::validate_json_schema(&request.json_schema)?;

        let existing = TemplateRepo::find_by_id(&self.db, ctx.workspace_id(), template_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))?;

        if existing.is_system {
            return Err(AppError::Forbidden(
                "Cannot update system templates".to_string(),
            ));
        }

        TemplateRepo::update(
            &self.db,
            ctx.workspace_id(),
            template_id,
            Some(&request.name),
            request.description.as_deref(),
            Some(&request.json_schema),
            Some(&request.prompt_template),
        )
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn delete(
        &self,
        ctx: &PrincipalContext,
        template_id: Uuid,
    ) -> Result<bool, AppError> {
        let existing = TemplateRepo::find_by_id(&self.db, ctx.workspace_id(), template_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))?;

        if existing.is_system {
            return Err(AppError::Forbidden(
                "Cannot delete system templates".to_string(),
            ));
        }

        TemplateRepo::delete(&self.db, ctx.workspace_id(), template_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    fn validate_json_schema(schema: &serde_json::Value) -> Result<(), AppError> {
        let obj = schema
            .as_object()
            .ok_or_else(|| AppError::Validation("json_schema must be a JSON object".to_string()))?;
        if !obj.contains_key("type") {
            return Err(AppError::Validation(
                "json_schema must have a 'type' field".to_string(),
            ));
        }
        if !obj.contains_key("properties") {
            return Err(AppError::Validation(
                "json_schema must have a 'properties' field".to_string(),
            ));
        }
        Ok(())
    }
}
