// SPDX-License-Identifier: AGPL-3.0-only

use std::fmt;

/// Persistence error for contract, evidence, and evaluation repositories.
#[derive(Debug)]
pub enum ContractStoreError {
    Database(sqlx::Error),
    Invalid(String),
    Conflict(String),
    NotFound(&'static str),
}

impl fmt::Display for ContractStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::Invalid(message) => write!(f, "{message}"),
            Self::Conflict(message) => write!(f, "{message}"),
            Self::NotFound(resource) => write!(f, "{resource} not found"),
        }
    }
}

impl std::error::Error for ContractStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Invalid(_) | Self::Conflict(_) | Self::NotFound(_) => None,
        }
    }
}

impl From<sqlx::Error> for ContractStoreError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(database) = &error {
            if database.code().as_deref() == Some("23505") {
                return Self::Conflict(error.to_string());
            }
        }
        Self::Database(error)
    }
}

impl ContractStoreError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

/// Map a decode failure onto `sqlx::Error::ColumnDecode`.
pub(crate) fn decode_error(
    column: &str,
    err: impl std::error::Error + Send + Sync + 'static,
) -> sqlx::Error {
    sqlx::Error::ColumnDecode {
        index: column.into(),
        source: Box::new(err),
    }
}
