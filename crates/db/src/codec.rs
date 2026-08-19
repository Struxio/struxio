// SPDX-License-Identifier: AGPL-3.0-only

//! Serde boundaries between `struxio-contracts` types and PostgreSQL.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::de::DeserializeOwned;
use serde_json::Value;
use struxio_contracts::{
    ContractSlug, ContractSpec, EvalDecision, EvidenceEntry, EvidenceKind, EvidenceSidecar,
    EvidenceSidecarStatus, ExtractionContract, JsonPointer, PositiveVersion, Sha256ContentHash,
    CANONICAL_HASH_VERSION,
};

use crate::error::{decode_error, ContractStoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalContractBytes {
    pub hash: Sha256ContentHash,
    pub payload: Vec<u8>,
    pub spec_json: Value,
}

pub(crate) fn encode_published_contract(
    contract: &ExtractionContract,
) -> Result<CanonicalContractBytes, ContractStoreError> {
    if !contract.verify_content_hash() {
        return Err(ContractStoreError::invalid(
            "refusing to persist a contract whose content hash does not match",
        ));
    }
    let payload = contract.canonical_payload();
    let hash = Sha256ContentHash::from_content(&payload);
    if hash != contract.content_hash() {
        return Err(ContractStoreError::invalid(
            "canonical payload is not the content-hash preimage",
        ));
    }
    let spec_json = serde_json::to_value(contract.spec()).map_err(|error| {
        ContractStoreError::invalid(format!("failed to serialize contract spec: {error}"))
    })?;
    Ok(CanonicalContractBytes {
        hash,
        payload,
        spec_json,
    })
}

pub(crate) fn decode_published_contract(
    slug: &str,
    version: i64,
    stored_hash: &str,
    payload: &[u8],
) -> Result<ExtractionContract, ContractStoreError> {
    let stored_hash: Sha256ContentHash = stored_hash
        .parse()
        .map_err(|error| ContractStoreError::invalid(format!("invalid content hash: {error}")))?;
    let payload_hash = Sha256ContentHash::from_content(payload);
    if payload_hash != stored_hash {
        return Err(ContractStoreError::invalid(
            "stored content hash does not match canonical payload",
        ));
    }

    let document: Value = serde_json::from_slice(payload).map_err(|error| {
        ContractStoreError::invalid(format!("canonical payload is not JSON: {error}"))
    })?;
    let version_number = document
        .get("canonical_hash_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| ContractStoreError::invalid("canonical payload missing hash version"))?;
    if version_number != u64::from(CANONICAL_HASH_VERSION) {
        return Err(ContractStoreError::invalid(format!(
            "unsupported canonical_hash_version {version_number}"
        )));
    }
    let spec_value = document
        .get("spec")
        .cloned()
        .ok_or_else(|| ContractStoreError::invalid("canonical payload missing spec"))?;
    let spec: ContractSpec = serde_json::from_value(spec_value).map_err(|error| {
        ContractStoreError::invalid(format!("canonical spec failed to decode: {error}"))
    })?;

    let slug = ContractSlug::new(slug)
        .map_err(|error| ContractStoreError::invalid(format!("invalid contract slug: {error}")))?;
    let version = u32::try_from(version)
        .map_err(|_| ContractStoreError::invalid("contract version does not fit u32"))?;
    let version = PositiveVersion::new(version).map_err(|error| {
        ContractStoreError::invalid(format!("invalid contract version: {error}"))
    })?;

    let contract = ExtractionContract::new(slug, version, spec);
    if contract.content_hash() != stored_hash {
        return Err(ContractStoreError::invalid(
            "decoded contract hash does not match stored content hash",
        ));
    }
    if contract.canonical_payload() != payload {
        return Err(ContractStoreError::invalid(
            "decoded contract bytes drifted from the stored canonical payload",
        ));
    }
    Ok(contract)
}

pub(crate) fn decode_json<T: DeserializeOwned>(
    column: &str,
    value: Value,
) -> Result<T, sqlx::Error> {
    serde_json::from_value(value).map_err(|error| decode_error(column, error))
}

pub(crate) fn encode_json<T: serde::Serialize>(
    column: &str,
    value: &T,
) -> Result<Value, sqlx::Error> {
    serde_json::to_value(value).map_err(|error| decode_error(column, error))
}

pub(crate) fn parse_pointer(value: &str) -> Result<JsonPointer, sqlx::Error> {
    JsonPointer::parse(value).map_err(|error| decode_error("json_pointer", error))
}

pub(crate) fn parse_enum<T: DeserializeOwned>(column: &str, value: &str) -> Result<T, sqlx::Error> {
    serde_json::from_value(Value::String(value.to_owned()))
        .map_err(|error| decode_error(column, error))
}

pub(crate) fn enum_str<T: serde::Serialize>(value: T) -> Result<String, sqlx::Error> {
    match serde_json::to_value(value).map_err(|error| decode_error("enum", error))? {
        Value::String(text) => Ok(text),
        other => Err(decode_error(
            "enum",
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("expected string enum, got {other}"),
            ),
        )),
    }
}

pub(crate) type StoredEvidenceRow = (String, i32, EvidenceKind, Option<String>, Option<String>);

pub(crate) fn group_evidence_entries(
    rows: Vec<StoredEvidenceRow>,
) -> Result<BTreeMap<JsonPointer, Vec<EvidenceEntry>>, sqlx::Error> {
    let mut entries: BTreeMap<JsonPointer, Vec<(i32, EvidenceEntry)>> = BTreeMap::new();
    for (pointer, ordinal, kind, quote, source) in rows {
        let pointer = parse_pointer(&pointer)?;
        let entry = EvidenceEntry::new(kind, quote, source)
            .map_err(|error| decode_error("quote", error))?;
        entries.entry(pointer).or_default().push((ordinal, entry));
    }
    let mut grouped = BTreeMap::new();
    for (pointer, mut items) in entries {
        items.sort_by_key(|(ordinal, _)| *ordinal);
        grouped.insert(pointer, items.into_iter().map(|(_, entry)| entry).collect());
    }
    Ok(grouped)
}

pub(crate) fn parse_decision(value: Option<String>) -> Result<Option<EvalDecision>, sqlx::Error> {
    value
        .map(|text| parse_enum("actual_decision", &text))
        .transpose()
}

pub(crate) fn parse_sidecar_status(value: &str) -> Result<EvidenceSidecarStatus, sqlx::Error> {
    parse_enum("status", value)
}

pub(crate) fn parse_hash(value: &str) -> Result<Sha256ContentHash, sqlx::Error> {
    Sha256ContentHash::from_str(value).map_err(|error| decode_error("content_hash", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use struxio_contracts::{
        EvidencePolicy, JsonSchema, JsonValueType, ProviderNeutralInstructions, Validator,
        ValidatorRule,
    };

    fn sample_contract() -> ExtractionContract {
        let name = JsonPointer::parse("/name").unwrap();
        let spec = ContractSpec::new(
            JsonSchema::new(serde_json::json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }))
            .unwrap(),
            ProviderNeutralInstructions::new("Extract the name").unwrap(),
        )
        .with_validator(
            ValidatorRule::new(name.clone(), Validator::Type(JsonValueType::String)).unwrap(),
        )
        .with_evidence_policy(EvidencePolicy::required(vec![name]));
        ExtractionContract::new(
            ContractSlug::new("person-name").unwrap(),
            PositiveVersion::new(1).unwrap(),
            spec,
        )
    }

    #[test]
    fn published_bytes_round_trip_and_reject_hash_mismatch() {
        let contract = sample_contract();
        let encoded = encode_published_contract(&contract).unwrap();
        assert_eq!(encoded.hash, contract.content_hash());

        let decoded = decode_published_contract(
            contract.identity().slug().as_str(),
            i64::from(contract.identity().version().get()),
            &encoded.hash.as_hex(),
            &encoded.payload,
        )
        .unwrap();
        assert_eq!(decoded, contract);

        let err = decode_published_contract(
            contract.identity().slug().as_str(),
            1,
            &encoded.hash.as_hex(),
            b"{\"canonical_hash_version\":1,\"spec\":{}}",
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not match"));
    }

    #[test]
    fn json_pointer_and_enum_boundaries() {
        let pointer = parse_pointer("/totals/0").unwrap();
        assert_eq!(pointer.to_string(), "/totals/0");
        assert!(parse_pointer("totals").is_err());
        assert_eq!(
            parse_enum::<EvidenceKind>("kind", "exact").unwrap(),
            EvidenceKind::Exact
        );
        assert_eq!(enum_str(EvidenceKind::Inferred).unwrap(), "inferred");
    }
}
