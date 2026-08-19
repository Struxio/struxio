// SPDX-License-Identifier: AGPL-3.0-only

use super::retry::Retryability;

/// Worker-facing error with an explicit retry classification.
///
/// Provider, storage, and timeout failures are retryable. Missing resources,
/// validation errors, and malformed payloads are permanent.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum JobError {
    #[error("{message}")]
    Retryable { message: String },
    #[error("{message}")]
    Permanent { message: String },
}

impl JobError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self::Retryable {
            message: message.into(),
        }
    }

    pub fn permanent(message: impl Into<String>) -> Self {
        Self::Permanent {
            message: message.into(),
        }
    }

    pub fn retryability(&self) -> Retryability {
        match self {
            Self::Retryable { .. } => Retryability::Retryable,
            Self::Permanent { .. } => Retryability::Permanent,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Retryable { message } | Self::Permanent { message } => message,
        }
    }
}

/// HTTP statuses that are safe to retry for an upstream provider call.
pub fn retryable_http_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
}
