// SPDX-License-Identifier: AGPL-3.0-only

use struxio_common::AppError;
use thiserror::Error;

/// Stable, agent-visible MCP error.
///
/// Application details such as workspace ids, row contents, and raw database
/// strings are stripped before they reach the protocol layer.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{message}")]
pub struct McpError {
    code: &'static str,
    message: String,
}

impl McpError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::new("invalid_arguments", message)
    }

    pub fn input_too_large(message: impl Into<String>) -> Self {
        Self::new("input_too_large", message)
    }

    pub fn tool_not_found() -> Self {
        Self::new("tool_not_found", "tool not found")
    }

    pub fn from_app(error: AppError) -> Self {
        match error {
            AppError::Auth(_) => Self::new("unauthorized", "authentication failed"),
            AppError::Forbidden(_) => Self::new("forbidden", "insufficient scope"),
            AppError::Validation(message) => Self::invalid_arguments(message),
            AppError::NotFound(_) => Self::new("not_found", "resource not found"),
            AppError::RateLimit(_) => Self::new("rate_limited", "request rate limit exceeded"),
            AppError::Duplicate(_) => Self::new("conflict", "resource already exists"),
            AppError::Database(_) => Self::new("internal_error", "database operation failed"),
            AppError::ExternalService(_) => Self::new("upstream_error", "upstream service failed"),
            AppError::Internal(_) => Self::new("internal_error", "internal server error"),
        }
    }

    pub fn into_envelope(self) -> ErrorEnvelope {
        ErrorEnvelope {
            error: ErrorDetail {
                code: self.code,
                message: self.message,
            },
        }
    }
}

pub type McpResult<T> = Result<T, McpError>;

/// Wire envelope placed in tool `isError` results.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub error: ErrorDetail,
}

/// Stable error object agents should parse.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ErrorDetail {
    pub code: &'static str,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_errors_use_stable_codes_and_do_not_leak_details() {
        let leaked = McpError::from_app(AppError::Database(
            "postgres://user:password@localhost/db".to_string(),
        ));
        assert_eq!(leaked.code(), "internal_error");
        assert_eq!(leaked.message(), "database operation failed");
        assert!(!leaked.message().contains("password"));

        let not_found = McpError::from_app(AppError::NotFound("extraction abc-secret".to_string()));
        assert_eq!(not_found.code(), "not_found");
        assert_eq!(not_found.message(), "resource not found");
        assert!(!not_found.message().contains("abc-secret"));

        let forbidden = McpError::from_app(AppError::Forbidden(
            "workspace 79ca2631-6230-5277-9d0e-879be839f744".to_string(),
        ));
        assert_eq!(forbidden.code(), "forbidden");
        assert_eq!(forbidden.message(), "insufficient scope");
        assert!(!forbidden.message().contains("79ca2631"));
    }
}
