use axum::{
    extract::{DefaultBodyLimit, Path, State},
    routing::{get, post},
    Json, Router,
};
use struxio_common::models::{CreateExtractionRequest, Extraction, InlineExtractionRequest};
use struxio_common::{PrincipalContext, Scope};
use struxio_core::services::extraction_service::MAX_DECODED_INLINE_BYTES;

use crate::errors::ApiError;
use crate::middleware::auth_provider::require_scope;
use crate::state::AppState;

const MAX_INLINE_REQUEST_BYTES: usize = ((MAX_DECODED_INLINE_BYTES + 2) / 3) * 4 + 64 * 1024;

pub async fn create_extraction(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Json(body): Json<CreateExtractionRequest>,
) -> Result<Json<Extraction>, ApiError> {
    require_scope(&ctx, Scope::ExtractionsCreate).map_err(ApiError)?;
    let model = state
        .model_service
        .get_default_model()
        .await
        .map_err(ApiError)?;

    let extraction = state
        .extraction_service
        .create_sync(&ctx, &body, &model.id)
        .await
        .map_err(ApiError)?;

    Ok(Json(extraction))
}

pub async fn create_inline_extraction(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Json(body): Json<InlineExtractionRequest>,
) -> Result<Json<Extraction>, ApiError> {
    require_scope(&ctx, Scope::ExtractionsCreate).map_err(ApiError)?;
    let model = state
        .model_service
        .get_default_model()
        .await
        .map_err(ApiError)?;

    let extraction = state
        .extraction_service
        .create_inline(&ctx, &body, &model.id)
        .await
        .map_err(ApiError)?;

    Ok(Json(extraction))
}

pub async fn list_extractions(
    ctx: PrincipalContext,
    State(state): State<AppState>,
) -> Result<Json<Vec<Extraction>>, ApiError> {
    require_scope(&ctx, Scope::ExtractionsRead).map_err(ApiError)?;
    let extractions = state
        .extraction_service
        .list(&ctx)
        .await
        .map_err(ApiError)?;
    Ok(Json(extractions))
}

pub async fn get_extraction(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<Json<Extraction>, ApiError> {
    require_scope(&ctx, Scope::ExtractionsRead).map_err(ApiError)?;
    let extraction = state
        .extraction_service
        .get(&ctx, path.id)
        .await
        .map_err(ApiError)?;
    Ok(Json(extraction))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_extractions).post(create_extraction))
        .route(
            "/inline",
            post(create_inline_extraction).layer(DefaultBodyLimit::max(MAX_INLINE_REQUEST_BYTES)),
        )
        .route("/{id}", get(get_extraction))
}
