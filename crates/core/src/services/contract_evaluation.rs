// SPDX-License-Identifier: AGPL-3.0-only

use sqlx::PgPool;
use struxio_common::{AppError, PrincipalContext};
use struxio_contracts::{EvalDecision, EvidenceSidecar, ExtractionContract, JsonPointer};
use struxio_db::records::{
    NewEvalRunResult, NewValidationReport, StoredContractVersion, StoredEvalRun,
    StoredEvidenceSidecar, StoredFixture, StoredValidationReport,
};
use struxio_db::repositories::contracts::ContractRepo;
use struxio_db::repositories::evaluations::{EvaluationRepo, ValidationRepo};
use struxio_db::repositories::evidence::EvidenceRepo;
use struxio_db::{compute_eval_run_metrics, ContractStoreError};
use uuid::Uuid;

pub struct ContractCatalogService {
    db: PgPool,
}

impl ContractCatalogService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn publish(
        &self,
        ctx: &PrincipalContext,
        contract: &ExtractionContract,
    ) -> Result<StoredContractVersion, AppError> {
        ContractRepo::publish(&self.db, ctx.workspace_id(), contract)
            .await
            .map_err(map_store_error)
    }

    pub async fn get(
        &self,
        ctx: &PrincipalContext,
        id: Uuid,
    ) -> Result<StoredContractVersion, AppError> {
        ContractRepo::find_by_id(&self.db, ctx.workspace_id(), id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::NotFound("contract version not found".to_string()))
    }

    pub async fn list_fixtures(
        &self,
        ctx: &PrincipalContext,
        contract_version_id: Uuid,
    ) -> Result<Vec<StoredFixture>, AppError> {
        let _ = self.get(ctx, contract_version_id).await?;
        ContractRepo::list_fixtures(&self.db, ctx.workspace_id(), contract_version_id)
            .await
            .map_err(map_store_error)
    }
}

pub struct EvaluationService {
    db: PgPool,
}

#[derive(Debug, Clone)]
pub struct FixtureOutcome {
    pub fixture_name: String,
    pub actual_data: serde_json::Value,
    pub actual_evidence: EvidenceSidecar,
    pub actual_decision: Option<EvalDecision>,
}

impl EvaluationService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Persist evidence beside an extraction without writing it into `result`.
    pub async fn record_extraction_evaluation(
        &self,
        ctx: &PrincipalContext,
        extraction_id: Uuid,
        contract_version_id: Uuid,
        data: &serde_json::Value,
        evidence: &EvidenceSidecar,
    ) -> Result<(StoredEvidenceSidecar, StoredValidationReport), AppError> {
        if data.get("evidence").is_some() {
            return Err(AppError::Validation(
                "extraction JSON must stay schema-pure; evidence belongs on the sidecar"
                    .to_string(),
            ));
        }
        let stored_contract =
            ContractRepo::find_by_id(&self.db, ctx.workspace_id(), contract_version_id)
                .await
                .map_err(map_store_error)?
                .ok_or_else(|| AppError::NotFound("contract version not found".to_string()))?;
        let evaluation = stored_contract
            .contract
            .evaluate_candidate(data, evidence)
            .map_err(|error| AppError::Validation(error.to_string()))?;
        let sidecar = EvidenceRepo::insert(&self.db, ctx.workspace_id(), evidence)
            .await
            .map_err(map_store_error)?;
        EvidenceRepo::attach_to_extraction(&self.db, ctx.workspace_id(), extraction_id, sidecar.id)
            .await
            .map_err(map_store_error)?;
        let report = ValidationRepo::insert(
            &self.db,
            ctx.workspace_id(),
            NewValidationReport {
                extraction_id,
                contract_version_id,
                sidecar_id: Some(sidecar.id),
                evaluation,
            },
        )
        .await
        .map_err(map_store_error)?;
        Ok((sidecar, report))
    }

    pub async fn record_eval_run(
        &self,
        ctx: &PrincipalContext,
        contract_version_id: Uuid,
        outcomes: &[FixtureOutcome],
    ) -> Result<StoredEvalRun, AppError> {
        let stored_contract =
            ContractRepo::find_by_id(&self.db, ctx.workspace_id(), contract_version_id)
                .await
                .map_err(map_store_error)?
                .ok_or_else(|| AppError::NotFound("contract version not found".to_string()))?;
        let fixtures =
            ContractRepo::list_fixtures(&self.db, ctx.workspace_id(), contract_version_id)
                .await
                .map_err(map_store_error)?;
        if fixtures.is_empty() {
            return Err(AppError::Validation(
                "contract version has no fixtures to evaluate".to_string(),
            ));
        }
        if outcomes.len() != fixtures.len() {
            return Err(AppError::Validation(
                "evaluation run must include exactly one outcome per fixture".to_string(),
            ));
        }

        let mut results = Vec::with_capacity(outcomes.len());
        for outcome in outcomes {
            if outcome.actual_data.get("evidence").is_some() {
                return Err(AppError::Validation(
                    "fixture actual JSON must stay schema-pure; evidence belongs on the sidecar"
                        .to_string(),
                ));
            }
            let fixture = fixtures
                .iter()
                .find(|item| item.descriptor.name() == outcome.fixture_name)
                .ok_or_else(|| {
                    AppError::Validation(format!("unknown fixture {}", outcome.fixture_name))
                })?;
            let evaluation = stored_contract
                .contract
                .evaluate_candidate(&outcome.actual_data, &outcome.actual_evidence)
                .map_err(|error| AppError::Validation(error.to_string()))?;
            let expected = stored_contract
                .contract
                .normalize_data(fixture.descriptor.expected_data())
                .map_err(|error| AppError::Validation(error.to_string()))?;
            let sidecar =
                EvidenceRepo::insert(&self.db, ctx.workspace_id(), &outcome.actual_evidence)
                    .await
                    .map_err(map_store_error)?;
            results.push(NewEvalRunResult {
                fixture_id: fixture.id,
                sidecar_id: sidecar.id,
                actual_data: outcome.actual_data.clone(),
                actual_decision: outcome.actual_decision,
                schema_valid: evaluation.schema().is_valid(),
                fields_match: evaluation.normalized() == &expected,
                evidence_satisfied: evaluation.evidence().is_satisfied(),
                schema_report: evaluation.schema().clone(),
                validation_report: evaluation.validation().clone(),
                evidence_report: evaluation.evidence().clone(),
            });
        }

        let metrics = compute_eval_run_metrics(stored_contract.contract.spec().eval(), &results);
        EvaluationRepo::insert_completed_run(
            &self.db,
            ctx.workspace_id(),
            contract_version_id,
            metrics,
            &results,
        )
        .await
        .map_err(map_store_error)
    }

    pub async fn evidence_for_pointer(
        &self,
        ctx: &PrincipalContext,
        sidecar_id: Uuid,
        pointer: &JsonPointer,
    ) -> Result<Vec<struxio_contracts::EvidenceEntry>, AppError> {
        EvidenceRepo::entries_for_pointer(&self.db, ctx.workspace_id(), sidecar_id, pointer)
            .await
            .map_err(map_store_error)
    }
}

fn map_store_error(error: ContractStoreError) -> AppError {
    match error {
        ContractStoreError::Database(error) => AppError::Database(error.to_string()),
        ContractStoreError::Invalid(message) => AppError::Validation(message),
        ContractStoreError::Conflict(message) => AppError::Duplicate(message),
        ContractStoreError::NotFound(resource) => {
            AppError::NotFound(format!("{resource} not found"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use struxio_contracts::EvalThresholds;
    use struxio_db::NewEvalRunResult;

    fn result(
        schema_valid: bool,
        fields_match: bool,
        evidence_satisfied: bool,
        abstain: bool,
    ) -> NewEvalRunResult {
        NewEvalRunResult {
            fixture_id: Uuid::nil(),
            sidecar_id: Uuid::nil(),
            actual_data: serde_json::json!({}),
            actual_decision: abstain.then_some(EvalDecision::Abstain),
            schema_valid,
            fields_match,
            evidence_satisfied,
            schema_report: struxio_contracts::JsonSchema::new(
                serde_json::json!({"type": "object"}),
            )
            .unwrap()
            .validate(&serde_json::json!({})),
            validation_report: struxio_contracts::ValidationReport::from_failures(Vec::new()),
            evidence_report: struxio_contracts::check_policy(
                &serde_json::json!({}),
                &EvidenceSidecar::not_requested(),
                &struxio_contracts::EvidencePolicy::none(),
            ),
        }
    }

    #[test]
    fn metrics_are_fail_closed_integer_basis_points() {
        let thresholds = EvalThresholds::new(10_000, 10_000, 5_000, 0).unwrap();
        let metrics = compute_eval_run_metrics(
            thresholds,
            &[
                result(true, true, true, false),
                result(true, false, true, false),
            ],
        );
        assert_eq!(metrics.schema_valid_rate_bps, 10_000);
        assert_eq!(metrics.field_accuracy_bps, 5_000);
        assert_eq!(metrics.evidence_coverage_bps, 10_000);
        assert_eq!(metrics.abstain_rate_bps, 0);
        assert!(!metrics.passed);
    }
}
