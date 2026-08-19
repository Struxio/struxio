// SPDX-License-Identifier: AGPL-3.0-only

use crate::error::ContractError;
use crate::evidence::EvidenceSidecar;
use crate::identity::Sha256ContentHash;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureError;

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fixture name or input descriptor is invalid")
    }
}

impl std::error::Error for FixtureError {}

/// Descriptor for a golden fixture. Binary inputs are referenced by hash, not embedded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureInput {
    file_name: String,
    media_type: String,
    content_sha256: Sha256ContentHash,
}

impl FixtureInput {
    pub fn new(
        file_name: impl Into<String>,
        media_type: impl Into<String>,
        content_sha256: Sha256ContentHash,
    ) -> Result<Self, FixtureError> {
        let file_name = file_name.into();
        let media_type = media_type.into();
        if file_name.trim().is_empty() || media_type.trim().is_empty() {
            return Err(FixtureError);
        }
        Ok(Self {
            file_name,
            media_type,
            content_sha256,
        })
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub fn content_sha256(&self) -> Sha256ContentHash {
        self.content_sha256
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalDecision {
    Accept,
    Review,
    Abstain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureDescriptor {
    name: String,
    input: FixtureInput,
    expected_data: Value,
    expected_evidence: EvidenceSidecar,
    expected_decision: Option<EvalDecision>,
}

impl FixtureDescriptor {
    pub fn new(
        name: impl Into<String>,
        input: FixtureInput,
        expected_data: Value,
        expected_evidence: EvidenceSidecar,
        expected_decision: Option<EvalDecision>,
    ) -> Result<Self, FixtureError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(FixtureError);
        }
        Ok(Self {
            name,
            input,
            expected_data,
            expected_evidence,
            expected_decision,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn input(&self) -> &FixtureInput {
        &self.input
    }

    pub fn expected_data(&self) -> &Value {
        &self.expected_data
    }

    pub fn expected_evidence(&self) -> &EvidenceSidecar {
        &self.expected_evidence
    }

    pub fn expected_decision(&self) -> Option<EvalDecision> {
        self.expected_decision
    }
}

/// Integer basis-point thresholds (0..=10000) so evaluation policy hashes stably.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalThresholds {
    min_schema_valid_rate_bps: u16,
    min_field_accuracy_bps: u16,
    min_evidence_coverage_bps: u16,
    max_abstain_rate_bps: u16,
}

impl EvalThresholds {
    pub fn new(
        min_schema_valid_rate_bps: u16,
        min_field_accuracy_bps: u16,
        min_evidence_coverage_bps: u16,
        max_abstain_rate_bps: u16,
    ) -> Result<Self, ContractError> {
        for (name, value) in [
            ("min_schema_valid_rate_bps", min_schema_valid_rate_bps),
            ("min_field_accuracy_bps", min_field_accuracy_bps),
            ("min_evidence_coverage_bps", min_evidence_coverage_bps),
            ("max_abstain_rate_bps", max_abstain_rate_bps),
        ] {
            if value > 10_000 {
                return Err(ContractError::Invalid(format!(
                    "{name} must be at most 10000 basis points"
                )));
            }
        }
        Ok(Self {
            min_schema_valid_rate_bps,
            min_field_accuracy_bps,
            min_evidence_coverage_bps,
            max_abstain_rate_bps,
        })
    }

    pub fn strict() -> Self {
        Self {
            min_schema_valid_rate_bps: 10_000,
            min_field_accuracy_bps: 10_000,
            min_evidence_coverage_bps: 10_000,
            max_abstain_rate_bps: 0,
        }
    }

    pub fn min_schema_valid_rate_bps(self) -> u16 {
        self.min_schema_valid_rate_bps
    }

    pub fn min_field_accuracy_bps(self) -> u16 {
        self.min_field_accuracy_bps
    }

    pub fn min_evidence_coverage_bps(self) -> u16 {
        self.min_evidence_coverage_bps
    }

    pub fn max_abstain_rate_bps(self) -> u16 {
        self.max_abstain_rate_bps
    }

    pub fn require_evidence(self) -> bool {
        self.min_evidence_coverage_bps > 0
    }
}

pub type EvalPolicy = EvalThresholds;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::EvidenceSidecar;

    #[test]
    fn eval_thresholds_use_integer_basis_points() {
        assert!(EvalThresholds::new(10_000, 9_000, 8_000, 500).is_ok());
        assert!(EvalThresholds::new(10_001, 0, 0, 0).is_err());
        assert_eq!(EvalThresholds::strict().min_schema_valid_rate_bps(), 10_000);
    }

    #[test]
    fn fixture_name_must_not_be_blank() {
        let input = FixtureInput::new(
            "basic_invoice.pdf",
            "application/pdf",
            Sha256ContentHash::from_content(b"pdf-bytes"),
        )
        .unwrap();
        assert!(FixtureDescriptor::new(
            "  ",
            input,
            serde_json::json!({}),
            EvidenceSidecar::not_requested(),
            None,
        )
        .is_err());
    }
}
