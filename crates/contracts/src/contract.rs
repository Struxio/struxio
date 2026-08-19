// SPDX-License-Identifier: AGPL-3.0-only

use crate::capabilities::{BackendCompatibility, BackendDescriptor};
use crate::error::ContractError;
use crate::evaluation::{EvalThresholds, FixtureDescriptor};
use crate::evidence::{check_policy, EvidencePolicy, EvidenceReport, EvidenceSidecar};
use crate::hash::{semantic_sha256, CANONICAL_HASH_VERSION};
use crate::identity::{ContractIdentity, ContractSlug, PositiveVersion, Sha256ContentHash};
use crate::instructions::ProviderNeutralInstructions;
use crate::normalization::{normalize, NormalizationError, NormalizerRule};
use crate::schema::{JsonSchema, SchemaValidationReport};
use crate::validation::{validate, ValidationFailure, ValidationReport, ValidatorRule};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mutable-looking builder for an immutable contract. There is no update path
/// on [`ExtractionContract`]; any change produces a new hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractSpec {
    schema: JsonSchema,
    instructions: ProviderNeutralInstructions,
    normalizers: Vec<NormalizerRule>,
    validators: Vec<ValidatorRule>,
    evidence_policy: EvidencePolicy,
    compatibility: BackendCompatibility,
    fixtures: Vec<FixtureDescriptor>,
    eval: EvalThresholds,
}

impl ContractSpec {
    pub fn new(schema: JsonSchema, instructions: ProviderNeutralInstructions) -> Self {
        Self {
            schema,
            instructions,
            normalizers: Vec::new(),
            validators: Vec::new(),
            evidence_policy: EvidencePolicy::none(),
            compatibility: BackendCompatibility::new(),
            fixtures: Vec::new(),
            eval: EvalThresholds::strict(),
        }
    }

    pub fn with_normalizer(mut self, rule: NormalizerRule) -> Self {
        self.normalizers.push(rule);
        self
    }

    pub fn with_validator(mut self, rule: ValidatorRule) -> Self {
        self.validators.push(rule);
        self
    }

    pub fn with_evidence_policy(mut self, policy: EvidencePolicy) -> Self {
        self.evidence_policy = policy;
        self
    }

    pub fn with_compatibility(mut self, compatibility: BackendCompatibility) -> Self {
        self.compatibility = compatibility;
        self
    }

    pub fn requiring_capability(mut self, capability: crate::BackendCapability) -> Self {
        self.compatibility = self.compatibility.requiring(capability);
        self
    }

    pub fn with_fixture(mut self, fixture: FixtureDescriptor) -> Self {
        self.fixtures.push(fixture);
        self
    }

    pub fn with_eval(mut self, eval: EvalThresholds) -> Self {
        self.eval = eval;
        self
    }

    pub fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    pub fn instructions(&self) -> &ProviderNeutralInstructions {
        &self.instructions
    }

    pub fn normalizers(&self) -> &[NormalizerRule] {
        &self.normalizers
    }

    pub fn validators(&self) -> &[ValidatorRule] {
        &self.validators
    }

    pub fn evidence_policy(&self) -> &EvidencePolicy {
        &self.evidence_policy
    }

    pub fn compatibility(&self) -> &BackendCompatibility {
        &self.compatibility
    }

    pub fn fixtures(&self) -> &[FixtureDescriptor] {
        &self.fixtures
    }

    pub fn eval(&self) -> EvalThresholds {
        self.eval
    }
}

/// Immutable extraction contract. Fields are private; identity is
/// `(slug, version, content_hash)` with no update operation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtractionContract {
    identity: ContractIdentity,
    spec: ContractSpec,
}

impl<'de> Deserialize<'de> for ExtractionContract {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireContract {
            identity: ContractIdentity,
            spec: ContractSpec,
        }

        let wire = WireContract::deserialize(deserializer)?;
        let expected = hash_contract(&wire.spec);
        if expected != wire.identity.content_hash() {
            return Err(serde::de::Error::custom(
                "content hash does not match canonical contract semantics",
            ));
        }
        if wire.identity.slug().as_str().is_empty() {
            return Err(serde::de::Error::custom("contract slug must not be empty"));
        }
        Ok(Self {
            identity: ContractIdentity::new(
                wire.identity.slug().clone(),
                wire.identity.version(),
                expected,
            ),
            spec: wire.spec,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateEvaluation {
    normalized: Value,
    schema: SchemaValidationReport,
    validation: ValidationReport,
    evidence: EvidenceReport,
}

impl CandidateEvaluation {
    pub fn is_valid(&self) -> bool {
        self.schema.is_valid() && self.validation.is_valid() && self.evidence.is_satisfied()
    }

    pub fn normalized(&self) -> &Value {
        &self.normalized
    }

    pub fn schema(&self) -> &SchemaValidationReport {
        &self.schema
    }

    pub fn validation(&self) -> &ValidationReport {
        &self.validation
    }

    pub fn evidence(&self) -> &EvidenceReport {
        &self.evidence
    }

    pub fn declarative_failures(&self) -> &[ValidationFailure] {
        self.validation.issues()
    }
}

impl ExtractionContract {
    pub fn new(slug: ContractSlug, version: PositiveVersion, spec: ContractSpec) -> Self {
        let content_hash = hash_contract(&spec);
        Self {
            identity: ContractIdentity::new(slug, version, content_hash),
            spec,
        }
    }

    pub fn identity(&self) -> &ContractIdentity {
        &self.identity
    }

    pub fn spec(&self) -> &ContractSpec {
        &self.spec
    }

    pub fn content_hash(&self) -> Sha256ContentHash {
        self.identity.content_hash()
    }

    pub fn verify_content_hash(&self) -> bool {
        self.content_hash() == hash_contract(&self.spec)
    }

    pub fn normalize_data(&self, data: &Value) -> Result<Value, NormalizationError> {
        normalize(data, &self.spec.normalizers)
    }

    /// Normalize, then validate schema, declarative rules, and evidence policy.
    /// Evidence is never written into `data`.
    pub fn evaluate_candidate(
        &self,
        data: &Value,
        evidence: &EvidenceSidecar,
    ) -> Result<CandidateEvaluation, ContractError> {
        let normalized = self.normalize_data(data)?;
        Ok(CandidateEvaluation {
            schema: self.spec.schema.validate(&normalized),
            validation: ValidationReport::from_failures(validate(
                &normalized,
                &self.spec.validators,
            )),
            evidence: check_policy(&normalized, evidence, &self.spec.evidence_policy),
            normalized,
        })
    }

    pub fn is_compatible_with(&self, backend: &BackendDescriptor) -> bool {
        self.spec.compatibility.is_satisfied_by(backend)
    }
}

fn hash_contract(spec: &ContractSpec) -> Sha256ContentHash {
    let semantic = serde_json::json!({
        "canonical_hash_version": CANONICAL_HASH_VERSION,
        "spec": spec,
    });
    semantic_sha256(&semantic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{BackendCapabilities, BackendCapability};
    use crate::evaluation::{EvalDecision, FixtureInput};
    use crate::evidence::{EvidenceEntry, EvidenceKind};
    use crate::pointer::JsonPointer;
    use crate::validation::{JsonValueType, Validator};

    fn sample_spec() -> ContractSpec {
        let name = JsonPointer::parse("/name").unwrap();
        ContractSpec::new(
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
        .with_evidence_policy(EvidencePolicy::required(vec![name]))
        .requiring_capability(BackendCapability::StructuredJson)
    }

    fn sample_contract() -> ExtractionContract {
        ExtractionContract::new(
            ContractSlug::new("person-name").unwrap(),
            PositiveVersion::new(1).unwrap(),
            sample_spec(),
        )
    }

    #[test]
    fn hash_ignores_object_key_order_in_schema() {
        let first_schema = JsonSchema::new(
            serde_json::from_str(
                r#"{"type":"object","properties":{"b":{"type":"number"},"a":{"type":"string"}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        let second_schema = JsonSchema::new(
            serde_json::from_str(
                r#"{"properties":{"a":{"type":"string"},"b":{"type":"number"}},"type":"object"}"#,
            )
            .unwrap(),
        )
        .unwrap();
        let first = ExtractionContract::new(
            ContractSlug::new("ordered").unwrap(),
            PositiveVersion::new(1).unwrap(),
            ContractSpec::new(
                first_schema,
                ProviderNeutralInstructions::new("task").unwrap(),
            ),
        );
        let second = ExtractionContract::new(
            ContractSlug::new("ordered").unwrap(),
            PositiveVersion::new(1).unwrap(),
            ContractSpec::new(
                second_schema,
                ProviderNeutralInstructions::new("task").unwrap(),
            ),
        );
        assert_eq!(first.content_hash(), second.content_hash());
    }

    #[test]
    fn slug_and_version_are_not_hashed() {
        let spec = sample_spec();
        let a = ExtractionContract::new(
            ContractSlug::new("alpha").unwrap(),
            PositiveVersion::new(1).unwrap(),
            spec.clone(),
        );
        let b = ExtractionContract::new(
            ContractSlug::new("beta").unwrap(),
            PositiveVersion::new(2).unwrap(),
            spec,
        );
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn any_semantic_field_change_changes_the_hash() {
        let base = sample_contract();
        let mut changed =
            sample_spec().with_eval(EvalThresholds::new(9_000, 9_000, 9_000, 100).unwrap());
        let eval_changed = ExtractionContract::new(
            base.identity().slug().clone(),
            base.identity().version(),
            changed.clone(),
        );
        assert_ne!(base.content_hash(), eval_changed.content_hash());

        changed = sample_spec().with_normalizer(NormalizerRule::new(
            JsonPointer::parse("/name").unwrap(),
            crate::Normalizer::TrimWhitespace,
        ));
        let norm_changed = ExtractionContract::new(
            base.identity().slug().clone(),
            base.identity().version(),
            changed.clone(),
        );
        assert_ne!(base.content_hash(), norm_changed.content_hash());

        changed = sample_spec().with_evidence_policy(EvidencePolicy::none());
        let evidence_changed = ExtractionContract::new(
            base.identity().slug().clone(),
            base.identity().version(),
            changed.clone(),
        );
        assert_ne!(base.content_hash(), evidence_changed.content_hash());

        changed = sample_spec().requiring_capability(BackendCapability::SourceEvidence);
        let compat_changed = ExtractionContract::new(
            base.identity().slug().clone(),
            base.identity().version(),
            changed.clone(),
        );
        assert_ne!(base.content_hash(), compat_changed.content_hash());

        let fixture = FixtureDescriptor::new(
            "basic",
            FixtureInput::new(
                "doc.pdf",
                "application/pdf",
                Sha256ContentHash::from_content(b"bytes"),
            )
            .unwrap(),
            serde_json::json!({"name": "Ada"}),
            EvidenceSidecar::not_requested(),
            Some(EvalDecision::Accept),
        )
        .unwrap();
        changed = sample_spec().with_fixture(fixture);
        let fixture_changed = ExtractionContract::new(
            base.identity().slug().clone(),
            base.identity().version(),
            changed,
        );
        assert_ne!(base.content_hash(), fixture_changed.content_hash());

        let instruction_changed = ExtractionContract::new(
            base.identity().slug().clone(),
            base.identity().version(),
            ContractSpec::new(
                base.spec().schema().clone(),
                ProviderNeutralInstructions::new("Different task").unwrap(),
            ),
        );
        assert_ne!(base.content_hash(), instruction_changed.content_hash());
    }

    #[test]
    fn serialization_round_trip_preserves_and_checks_hash() {
        let contract = sample_contract();
        assert!(contract.verify_content_hash());
        let encoded = serde_json::to_string(&contract).unwrap();
        let decoded: ExtractionContract = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, contract);

        let mut tampered: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        tampered["identity"]["content_hash"] = serde_json::Value::String("00".repeat(32));
        assert!(serde_json::from_value::<ExtractionContract>(tampered).is_err());
    }

    #[test]
    fn evaluates_schema_validators_and_required_evidence() {
        let contract = sample_contract();
        let data = serde_json::json!({"name": "Ada"});
        let missing = contract
            .evaluate_candidate(&data, &EvidenceSidecar::from_entries(Default::default()))
            .unwrap();
        assert!(!missing.is_valid());

        let evidence = EvidenceSidecar::from_entries(Default::default()).with(
            JsonPointer::parse("/name").unwrap(),
            EvidenceEntry::new(EvidenceKind::Exact, Some("Ada".into()), Some("p1".into())).unwrap(),
        );
        let ok = contract.evaluate_candidate(&data, &evidence).unwrap();
        assert!(ok.is_valid());
        assert_eq!(ok.normalized()["name"], "Ada");
        assert!(ok.normalized().get("evidence").is_none());
    }

    #[test]
    fn required_evidence_unavailable_fails_closed() {
        let contract = sample_contract();
        let data = serde_json::json!({"name": "Ada"});
        let report = contract
            .evaluate_candidate(&data, &EvidenceSidecar::unavailable())
            .unwrap();
        assert!(!report.is_valid());
    }

    #[test]
    fn backend_without_required_capability_is_incompatible() {
        let contract = sample_contract()
            .spec()
            .clone()
            .requiring_capability(BackendCapability::SourceEvidence);
        let contract = ExtractionContract::new(
            ContractSlug::new("person-name").unwrap(),
            PositiveVersion::new(1).unwrap(),
            contract,
        );
        let direct = BackendDescriptor::new(
            "eval-provider",
            "direct",
            BackendCapabilities::new()
                .with(BackendCapability::StructuredJson)
                .with(BackendCapability::RawBytes),
        );
        assert!(!contract.is_compatible_with(&direct));
    }
}
