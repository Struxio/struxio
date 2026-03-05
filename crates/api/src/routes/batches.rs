use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use struxio_common::models::{BatchJob, CreateBatchRequest, Extraction};

use crate::errors::ApiError;
use crate::middleware::auth_provider::AuthUser;
use crate::state::AppState;

pub async fn create_batch(
    _auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateBatchRequest>,
) -> Result<Json<BatchJob>, ApiError> {
    let model = state
        .model_service
        .get_default_model()
        .await
        .map_err(ApiError)?;

    let batch = state
        .batch_service
        .create(&body, &model.id)
        .await
        .map_err(ApiError)?;

    Ok(Json(batch))
}

pub async fn list_batches(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<BatchJob>>, ApiError> {
    let batches = state.batch_service.list().await.map_err(ApiError)?;
    Ok(Json(batches))
}

pub async fn get_batch(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<Json<BatchJob>, ApiError> {
    let batch = state.batch_service.get(path.id).await.map_err(ApiError)?;
    Ok(Json(batch))
}

pub async fn list_batch_extractions(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<Json<Vec<Extraction>>, ApiError> {
    let extractions = state
        .batch_service
        .list_extractions(path.id)
        .await
        .map_err(ApiError)?;
    Ok(Json(extractions))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_batches).post(create_batch))
        .route("/{id}", get(get_batch))
        .route("/{id}/extractions", get(list_batch_extractions))
}
