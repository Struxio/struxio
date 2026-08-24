// SPDX-License-Identifier: AGPL-3.0-only

//! Immutable extraction-contract domain types.
//!
//! This crate has no HTTP, database, queue, cloud, or vendor-provider dependencies.
//! Extracted JSON stays schema-pure; evidence lives only on a JSON-Pointer sidecar.

mod canonical;
mod capabilities;
mod contract;
mod error;
mod evaluation;
mod evidence;
mod hash;
mod identity;
mod instructions;
mod normalization;
mod pointer;
mod schema;
mod validation;

pub use capabilities::{
    BackendCapabilities, BackendCapability, BackendCompatibility, BackendDescriptor,
};
pub use contract::{CandidateEvaluation, ContractSpec, ExtractionContract};
pub use error::ContractError;
pub use evaluation::{
    EvalDecision, EvalPolicy, EvalThresholds, FixtureDescriptor, FixtureError, FixtureInput,
};
pub use evidence::{
    check_policy, EvidenceCheckStatus, EvidenceEntry, EvidenceError, EvidenceKind,
    EvidencePathStatus, EvidencePolicy, EvidenceReport, EvidenceSidecar, EvidenceSidecarStatus,
    EVIDENCE_SIDECAR_VERSION,
};
pub use hash::{semantic_sha256, CANONICAL_HASH_VERSION};
pub use identity::{
    ContractIdentity, ContractSlug, IdentityError, PositiveVersion, Sha256ContentHash,
};
pub use instructions::{InstructionsError, ProviderNeutralInstructions};
pub use normalization::{normalize, NormalizationError, Normalizer, NormalizerRule};
pub use pointer::{JsonPointer, JsonPointerError};
pub use schema::{JsonSchema, SchemaError, SchemaValidationError, SchemaValidationReport};
pub use validation::{
    validate, JsonValueType, ValidationFailure, ValidationReport, ValidationStatus, Validator,
    ValidatorRule,
};

pub use canonical::canonical_json;
