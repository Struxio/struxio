use axum::{extract::State, routing::get, Json, Router};
use struxio_common::models::AiModel;

use struxio_common::PrincipalContext;

use crate::errors::ApiError;
use crate::state::AppState;

pub async fn get_default_model(
    _ctx: PrincipalContext,
    State(state): State<AppState>,
) -> Result<Json<AiModel>, ApiError> {
    let model = state
        .model_service
        .get_default_model()
        .await
        .map_err(ApiError)?;
    Ok(Json(model))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/default", get(get_default_model))
}
