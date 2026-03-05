use axum::{extract::FromRequestParts, http::header, http::request::Parts};
use struxio_common::AppError;

use crate::errors::ApiError;
use crate::middleware::auth_provider::AuthUser;
use crate::state::AppState;

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self, ApiError> {
        // Cloud binary injects AuthUser via request extensions (Clerk JWT middleware).
        if parts.extensions.get::<AuthUser>().is_some() {
            return Ok(AuthUser);
        }

        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError(AppError::Auth("Missing Authorization header".to_string())))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError(AppError::Auth("Invalid Authorization header format".to_string())))?;

        let expected_key = std::env::var("STRUXIO_API_KEY").unwrap_or_default();
        if expected_key.is_empty() || token != expected_key {
            return Err(ApiError(AppError::Auth("Invalid API key".to_string())));
        }

        Ok(AuthUser)
    }
}
