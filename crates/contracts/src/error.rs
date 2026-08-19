// SPDX-License-Identifier: AGPL-3.0-only

use crate::normalization::NormalizationError;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    Invalid(String),
    InvalidRegex(String),
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => message.fmt(f),
            Self::InvalidRegex(message) => write!(f, "invalid regular expression: {message}"),
        }
    }
}

impl std::error::Error for ContractError {}

impl From<NormalizationError> for ContractError {
    fn from(error: NormalizationError) -> Self {
        Self::Invalid(error.to_string())
    }
}
