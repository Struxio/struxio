#![allow(dead_code)]

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use struxio_common::AppError;

#[derive(Debug)]
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match &self.0 {
            AppError::Auth(_) => (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({ "error": self.0.to_string() })),
            ),
            AppError::Forbidden(msg) => {
                if let Some(rest) = msg.strip_prefix("api_key_limit_reached:") {
                    if let Ok(n) = rest.parse::<i32>() {
                        return (
                            StatusCode::FORBIDDEN,
                            axum::Json(serde_json::json!({
                                "error": "api_key_limit_reached",
                                "message": format!(
                                    "Your plan allows a maximum of {} API keys. Upgrade your plan to create more.",
                                    n
                                )
                            })),
                        )
                            .into_response();
                    }
                }
                (
                    StatusCode::FORBIDDEN,
                    axum::Json(serde_json::json!({ "error": msg })),
                )
            }
            AppError::NotFound(_) => (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({ "error": self.0.to_string() })),
            ),
            AppError::Validation(_) => (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": self.0.to_string() })),
            ),
            AppError::RateLimit(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                axum::Json(serde_json::json!({ "error": self.0.to_string() })),
            ),
            AppError::Duplicate(_) => (
                StatusCode::CONFLICT,
                axum::Json(serde_json::json!({ "error": self.0.to_string() })),
            ),
            e => {
                tracing::error!("Internal server error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(serde_json::json!({ "error": "Internal server error" })),
                )
            }
        };
        (status, body).into_response()
    }
}
