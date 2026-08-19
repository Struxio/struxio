use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use struxio_common::models::{CreateTemplateRequest, ExtractionTemplate, UpdateTemplateRequest};
use struxio_common::{PrincipalContext, Scope};

use crate::errors::ApiError;
use crate::middleware::auth_provider::require_scope;
use crate::state::AppState;

pub async fn list_templates(
    ctx: PrincipalContext,
    State(state): State<AppState>,
) -> Result<Json<Vec<ExtractionTemplate>>, ApiError> {
    require_scope(&ctx, Scope::TemplatesRead).map_err(ApiError)?;
    let templates = state.template_service.list(&ctx).await.map_err(ApiError)?;
    Ok(Json(templates))
}

pub async fn get_template(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<Json<ExtractionTemplate>, ApiError> {
    require_scope(&ctx, Scope::TemplatesRead).map_err(ApiError)?;
    let template = state
        .template_service
        .get(&ctx, path.id)
        .await
        .map_err(ApiError)?;
    Ok(Json(template))
}

pub async fn create_template(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Json(body): Json<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<ExtractionTemplate>), ApiError> {
    require_scope(&ctx, Scope::TemplatesWrite).map_err(ApiError)?;
    let template = state
        .template_service
        .create(&ctx, &body)
        .await
        .map_err(ApiError)?;
    Ok((StatusCode::CREATED, Json(template)))
}

pub async fn update_template(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
    Json(body): Json<UpdateTemplateRequest>,
) -> Result<Json<ExtractionTemplate>, ApiError> {
    require_scope(&ctx, Scope::TemplatesWrite).map_err(ApiError)?;
    let template = state
        .template_service
        .update(&ctx, path.id, &body)
        .await
        .map_err(ApiError)?;
    Ok(Json(template))
}

pub async fn delete_template(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<StatusCode, ApiError> {
    require_scope(&ctx, Scope::TemplatesWrite).map_err(ApiError)?;
    let deleted = state
        .template_service
        .delete(&ctx, path.id)
        .await
        .map_err(ApiError)?;
    if !deleted {
        return Err(ApiError(struxio_common::AppError::NotFound(
            "Template not found".to_string(),
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_templates).post(create_template))
        .route(
            "/{id}",
            get(get_template)
                .put(update_template)
                .delete(delete_template),
        )
}
