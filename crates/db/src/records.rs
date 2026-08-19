// SPDX-License-Identifier: AGPL-3.0-only

use chrono::{DateTime, Utc};
use serde_json::Value;
use struxio_common::WorkspaceId;
use struxio_contracts::{
    CandidateEvaluation, EvalDecision, EvidenceReport, EvidenceSidecar, ExtractionContract,
    FixtureDescriptor, SchemaValidationReport, ValidationReport,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct StoredContractVersion {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub contract: ExtractionContract,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredFixture {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub contract_version_id: Uuid,
    pub descriptor: FixtureDescriptor,
    pub expected_sidecar_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvidenceSidecar {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub sidecar: EvidenceSidecar,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvidenceAttachment {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub extraction_id: Uuid,
    pub sidecar_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredValidationReport {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub extraction_id: Uuid,
    pub contract_version_id: Uuid,
    pub sidecar_id: Option<Uuid>,
    pub evaluation: CandidateEvaluation,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewValidationReport {
    pub extraction_id: Uuid,
    pub contract_version_id: Uuid,
    pub sidecar_id: Option<Uuid>,
    pub evaluation: CandidateEvaluation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvalRun {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub contract_version_id: Uuid,
    pub schema_valid_rate_bps: i32,
    pub field_accuracy_bps: i32,
    pub evidence_coverage_bps: i32,
    pub abstain_rate_bps: i32,
    pub passed: bool,
    pub created_at: DateTime<Utc>,
    pub results: Vec<StoredEvalRunResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvalRunResult {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub eval_run_id: Uuid,
    pub fixture_id: Uuid,
    pub sidecar_id: Uuid,
    pub actual_data: Value,
    pub actual_decision: Option<EvalDecision>,
    pub schema_valid: bool,
    pub fields_match: bool,
    pub evidence_satisfied: bool,
    pub schema_report: SchemaValidationReport,
    pub validation_report: ValidationReport,
    pub evidence_report: EvidenceReport,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewEvalRunResult {
    pub fixture_id: Uuid,
    pub sidecar_id: Uuid,
    pub actual_data: Value,
    pub actual_decision: Option<EvalDecision>,
    pub schema_valid: bool,
    pub fields_match: bool,
    pub evidence_satisfied: bool,
    pub schema_report: SchemaValidationReport,
    pub validation_report: ValidationReport,
    pub evidence_report: EvidenceReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalRunMetrics {
    pub schema_valid_rate_bps: i32,
    pub field_accuracy_bps: i32,
    pub evidence_coverage_bps: i32,
    pub abstain_rate_bps: i32,
    pub passed: bool,
}
