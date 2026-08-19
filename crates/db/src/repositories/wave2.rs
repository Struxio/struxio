use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::Row;
use struxio_contracts::{semantic_sha256, Sha256ContentHash};

#[derive(Debug)]
pub(crate) struct DomainDecodeError(String);

impl std::fmt::Display for DomainDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for DomainDecodeError {}

pub(crate) fn domain_error(message: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(DomainDecodeError(message.into())))
}

pub(crate) fn to_json<T: Serialize>(column: &str, value: &T) -> Result<Value, sqlx::Error> {
    serde_json::to_value(value)
        .map_err(|error| domain_error(format!("failed to serialize {column}: {error}")))
}

pub(crate) fn from_json<T: DeserializeOwned>(column: &str, value: Value) -> Result<T, sqlx::Error> {
    serde_json::from_value(value)
        .map_err(|error| domain_error(format!("failed to decode {column}: {error}")))
}

pub(crate) fn hash_json(value: &Value) -> Sha256ContentHash {
    semantic_sha256(value)
}

pub(crate) fn content_hash_bytes(hash: Sha256ContentHash) -> Vec<u8> {
    hash.as_bytes().to_vec()
}

pub(crate) fn hash_from_row(row: &PgRow, column: &str) -> Result<Sha256ContentHash, sqlx::Error> {
    let bytes: Vec<u8> = row.try_get(column)?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| domain_error(format!("{column} must contain a 32-byte SHA-256 digest")))?;
    Ok(Sha256ContentHash::from_bytes(bytes))
}

pub(crate) fn optional_decision(
    value: Option<String>,
) -> Result<Option<struxio_contracts::EvalDecision>, sqlx::Error> {
    value
        .map(|value| match value.as_str() {
            "accept" => Ok(struxio_contracts::EvalDecision::Accept),
            "review" => Ok(struxio_contracts::EvalDecision::Review),
            "abstain" => Ok(struxio_contracts::EvalDecision::Abstain),
            _ => Err(domain_error(format!("unknown evaluation decision {value}"))),
        })
        .transpose()
}

pub(crate) fn decision_text(
    value: Option<struxio_contracts::EvalDecision>,
) -> Option<&'static str> {
    value.map(|decision| match decision {
        struxio_contracts::EvalDecision::Accept => "accept",
        struxio_contracts::EvalDecision::Review => "review",
        struxio_contracts::EvalDecision::Abstain => "abstain",
    })
}
