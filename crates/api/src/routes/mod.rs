use axum::{
    routing::{get, post},
    Router,
};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct IdPath {
    pub id: Uuid,
}

use crate::state::AppState;

mod batches;
mod documents;
mod extractions;
mod models;
mod templates;

pub fn oss_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest("/templates", templates::router())
        .nest("/batches", batches::router())
        .nest("/extractions", extractions::router())
        .nest("/models", models::router())
        .nest(
            "/documents",
            Router::new()
                .route("/check", post(documents::check_document))
                .route("/confirm", post(documents::confirm_upload))
                .route("/", get(documents::list_documents))
                .route("/{id}", get(documents::get_document).delete(documents::delete_document)),
        )
}
