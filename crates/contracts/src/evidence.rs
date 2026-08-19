// SPDX-License-Identifier: AGPL-3.0-only

//! Evidence policy and JSON-Pointer sidecar. Extracted JSON stays schema-pure;
//! citations live only in the sidecar and must never be wrapped into field values.

use crate::pointer::JsonPointer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

pub const EVIDENCE_SIDECAR_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceError(String);

impl fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for EvidenceError {}

impl EvidenceError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Exact,
    Normalized,
    Computed,
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSidecarStatus {
    NotRequested,
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceEntry {
    kind: EvidenceKind,
    quote: Option<String>,
    source: Option<String>,
}

impl EvidenceEntry {
    pub fn new(
        kind: EvidenceKind,
        quote: Option<String>,
        source: Option<String>,
    ) -> Result<Self, EvidenceError> {
        if let Some(quote) = &quote {
            if quote.trim().is_empty() {
                return Err(EvidenceError::new("evidence quote must not be empty"));
            }
        }
        Ok(Self { kind, quote, source })
    }

    pub fn kind(&self) -> EvidenceKind {
        self.kind
    }

    pub fn quote(&self) -> Option<&str> {
        self.quote.as_deref()
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn supports_required_path(&self) -> bool {
        matches!(self.kind, EvidenceKind::Exact | EvidenceKind::Normalized)
    }
}

/// JSON-Pointer-keyed sidecar. Keys are stored in a `BTreeMap` so serialization
/// order is canonical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSidecar {
    version: u16,
    status: EvidenceSidecarStatus,
    entries: BTreeMap<JsonPointer, Vec<EvidenceEntry>>,
}

impl Default for EvidenceSidecar {
    fn default() -> Self {
        Self::not_requested()
    }
}

impl EvidenceSidecar {
    pub fn not_requested() -> Self {
        Self {
            version: EVIDENCE_SIDECAR_VERSION,
            status: EvidenceSidecarStatus::NotRequested,
            entries: BTreeMap::new(),
        }
    }

    pub fn unavailable() -> Self {
        Self {
            version: EVIDENCE_SIDECAR_VERSION,
            status: EvidenceSidecarStatus::Unavailable,
            entries: BTreeMap::new(),
        }
    }

    pub fn from_entries(entries: BTreeMap<JsonPointer, Vec<EvidenceEntry>>) -> Self {
        let status = if entries.is_empty() {
            EvidenceSidecarStatus::Partial
        } else {
            EvidenceSidecarStatus::Complete
        };
        Self {
            version: EVIDENCE_SIDECAR_VERSION,
            status,
            entries,
        }
    }

    pub fn with(mut self, pointer: JsonPointer, entry: EvidenceEntry) -> Self {
        self.entries.entry(pointer).or_default().push(entry);
        if self.status == EvidenceSidecarStatus::NotRequested {
            self.status = EvidenceSidecarStatus::Partial;
        }
        self
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn status(&self) -> EvidenceSidecarStatus {
        self.status
    }

    pub fn entries(&self) -> &BTreeMap<JsonPointer, Vec<EvidenceEntry>> {
        &self.entries
    }

    pub fn get(&self, pointer: &JsonPointer) -> Option<&[EvidenceEntry]> {
        self.entries.get(pointer).map(Vec::as_slice)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Contract-level evidence policy. Required evidence fails closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "paths")]
pub enum EvidencePolicy {
    None,
    Optional(Vec<JsonPointer>),
    Required(Vec<JsonPointer>),
}

impl EvidencePolicy {
    pub fn none() -> Self {
        Self::None
    }

    pub fn optional(paths: Vec<JsonPointer>) -> Self {
        Self::Optional(paths)
    }

    pub fn required(paths: Vec<JsonPointer>) -> Self {
        Self::Required(paths)
    }

    pub fn paths(&self) -> &[JsonPointer] {
        match self {
            Self::None => &[],
            Self::Optional(paths) | Self::Required(paths) => paths,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceCheckStatus {
    NotRequested,
    Satisfied,
    MissingOptional,
    MissingRequired,
    Unavailable,
    InferredOnly,
    DataMissing,
}

impl EvidenceCheckStatus {
    pub fn is_fail_closed(self) -> bool {
        matches!(
            self,
            Self::MissingRequired | Self::Unavailable | Self::InferredOnly | Self::DataMissing
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidencePathStatus {
    pointer: JsonPointer,
    status: EvidenceCheckStatus,
}

impl EvidencePathStatus {
    pub fn pointer(&self) -> &JsonPointer {
        &self.pointer
    }

    pub fn status(&self) -> EvidenceCheckStatus {
        self.status
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceReport {
    statuses: Vec<EvidencePathStatus>,
}

impl EvidenceReport {
    pub fn statuses(&self) -> &[EvidencePathStatus] {
        &self.statuses
    }

    pub fn is_satisfied(&self) -> bool {
        !self.statuses.iter().any(|item| item.status.is_fail_closed())
    }
}

fn path_present(data: &Value, pointer: &JsonPointer) -> bool {
    matches!(pointer.get(data), Ok(Some(value)) if !value.is_null())
}

fn path_status(
    pointer: &JsonPointer,
    data: &Value,
    sidecar: &EvidenceSidecar,
    required: bool,
) -> EvidenceCheckStatus {
    if sidecar.status == EvidenceSidecarStatus::Unavailable {
        return if required {
            EvidenceCheckStatus::Unavailable
        } else {
            EvidenceCheckStatus::MissingOptional
        };
    }
    if sidecar.status == EvidenceSidecarStatus::NotRequested {
        return if required {
            EvidenceCheckStatus::MissingRequired
        } else {
            EvidenceCheckStatus::NotRequested
        };
    }
    if !path_present(data, pointer) {
        return if required {
            EvidenceCheckStatus::DataMissing
        } else {
            EvidenceCheckStatus::MissingOptional
        };
    }
    match sidecar.get(pointer) {
        Some(entries) if entries.iter().any(EvidenceEntry::supports_required_path) => {
            EvidenceCheckStatus::Satisfied
        }
        Some(entries) if !entries.is_empty() => {
            if required {
                EvidenceCheckStatus::InferredOnly
            } else {
                EvidenceCheckStatus::Satisfied
            }
        }
        _ => {
            if required {
                EvidenceCheckStatus::MissingRequired
            } else {
                EvidenceCheckStatus::MissingOptional
            }
        }
    }
}

pub fn check_policy(data: &Value, sidecar: &EvidenceSidecar, policy: &EvidencePolicy) -> EvidenceReport {
    let statuses = match policy {
        EvidencePolicy::None => Vec::new(),
        EvidencePolicy::Optional(paths) => paths
            .iter()
            .map(|pointer| EvidencePathStatus {
                pointer: pointer.clone(),
                status: path_status(pointer, data, sidecar, false),
            })
            .collect(),
        EvidencePolicy::Required(paths) => {
            if sidecar.status == EvidenceSidecarStatus::Unavailable {
                return EvidenceReport {
                    statuses: vec![EvidencePathStatus {
                        pointer: JsonPointer::root(),
                        status: EvidenceCheckStatus::Unavailable,
                    }],
                };
            }
            paths
                .iter()
                .map(|pointer| EvidencePathStatus {
                    pointer: pointer.clone(),
                    status: path_status(pointer, data, sidecar, true),
                })
                .collect()
        }
    };
    EvidenceReport { statuses }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total() -> JsonPointer {
        JsonPointer::parse("/total").unwrap()
    }

    #[test]
    fn required_evidence_fails_closed_when_missing() {
        let policy = EvidencePolicy::required(vec![total()]);
        let report = check_policy(
            &serde_json::json!({"total": 10}),
            &EvidenceSidecar::from_entries(BTreeMap::new()),
            &policy,
        );
        assert!(!report.is_satisfied());
        assert_eq!(report.statuses()[0].status(), EvidenceCheckStatus::MissingRequired);
    }

    #[test]
    fn required_evidence_fails_closed_when_unavailable() {
        let policy = EvidencePolicy::required(vec![total()]);
        let report = check_policy(
            &serde_json::json!({"total": 10}),
            &EvidenceSidecar::unavailable(),
            &policy,
        );
        assert!(!report.is_satisfied());
        assert_eq!(report.statuses()[0].status(), EvidenceCheckStatus::Unavailable);
    }

    #[test]
    fn inferred_evidence_cannot_satisfy_a_required_path() {
        let pointer = total();
        let sidecar = EvidenceSidecar::from_entries(BTreeMap::new()).with(
            pointer.clone(),
            EvidenceEntry::new(EvidenceKind::Inferred, Some("maybe 10".into()), None).unwrap(),
        );
        let policy = EvidencePolicy::required(vec![pointer]);
        let report = check_policy(&serde_json::json!({"total": 10}), &sidecar, &policy);
        assert_eq!(report.statuses()[0].status(), EvidenceCheckStatus::InferredOnly);
        assert!(!report.is_satisfied());
    }

    #[test]
    fn exact_evidence_satisfies_required_path() {
        let pointer = total();
        let sidecar = EvidenceSidecar::from_entries(BTreeMap::new()).with(
            pointer.clone(),
            EvidenceEntry::new(EvidenceKind::Exact, Some("10".into()), Some("p1".into())).unwrap(),
        );
        let policy = EvidencePolicy::required(vec![pointer]);
        assert!(check_policy(&serde_json::json!({"total": 10}), &sidecar, &policy).is_satisfied());
    }

    #[test]
    fn optional_missing_evidence_is_not_fail_closed() {
        let policy = EvidencePolicy::optional(vec![total()]);
        let report = check_policy(
            &serde_json::json!({"total": 10}),
            &EvidenceSidecar::from_entries(BTreeMap::new()),
            &policy,
        );
        assert!(report.is_satisfied());
        assert_eq!(
            report.statuses()[0].status(),
            EvidenceCheckStatus::MissingOptional
        );
    }
}
