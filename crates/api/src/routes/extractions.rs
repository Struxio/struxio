use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use struxio_common::models::{CreateExtractionRequest, Extraction, InlineExtractionRequest};

use crate::errors::ApiError;
use crate::middleware::auth_provider::AuthUser;
use crate::state::AppState;

pub async fn create_extraction(
    _auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateExtractionRequest>,
) -> Result<Json<Extraction>, ApiError> {
    let model = state
        .model_service
        .get_default_model()
        .await
        .map_err(ApiError)?;

    let extraction = state
        .extraction_service
        .create_sync(&body, &model.id)
        .await
        .map_err(ApiError)?;

    Ok(Json(extraction))
}

pub async fn create_inline_extraction(
    _auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<InlineExtractionRequest>,
) -> Result<Json<Extraction>, ApiError> {
    let model = state
        .model_service
        .get_default_model()
        .await
        .map_err(ApiError)?;

    let extraction = state
        .extraction_service
        .create_inline(&body, &model.id)
        .await
        .map_err(ApiError)?;

    Ok(Json(extraction))
}

pub async fn list_extractions(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Extraction>>, ApiError> {
    let extractions = state
        .extraction_service
        .list()
        .await
        .map_err(ApiError)?;
    Ok(Json(extractions))
}

pub async fn get_extraction(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<Json<Extraction>, ApiError> {
    let extraction = state
        .extraction_service
        .get(path.id)
        .await
        .map_err(ApiError)?;
    Ok(Json(extraction))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_extractions).post(create_extraction))
        .route("/inline", post(create_inline_extraction))
        .route("/{id}", get(get_extraction))
}
