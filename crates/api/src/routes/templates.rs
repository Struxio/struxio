use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use struxio_common::models::{CreateTemplateRequest, ExtractionTemplate, UpdateTemplateRequest};

use crate::errors::ApiError;
use crate::middleware::auth_provider::AuthUser;
use crate::state::AppState;

pub async fn list_templates(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ExtractionTemplate>>, ApiError> {
    let templates = state
        .template_service
        .list()
        .await
        .map_err(ApiError)?;
    Ok(Json(templates))
}

pub async fn get_template(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<Json<ExtractionTemplate>, ApiError> {
    let template = state
        .template_service
        .get(path.id)
        .await
        .map_err(ApiError)?;
    Ok(Json(template))
}

pub async fn create_template(
    _auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<ExtractionTemplate>), ApiError> {
    let template = state
        .template_service
        .create(&body)
        .await
        .map_err(ApiError)?;
    Ok((StatusCode::CREATED, Json(template)))
}

pub async fn update_template(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
    Json(body): Json<UpdateTemplateRequest>,
) -> Result<Json<ExtractionTemplate>, ApiError> {
    let template = state
        .template_service
        .update(path.id, &body)
        .await
        .map_err(ApiError)?;
    Ok(Json(template))
}

pub async fn delete_template(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<StatusCode, ApiError> {
    let deleted = state
        .template_service
        .delete(path.id)
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
