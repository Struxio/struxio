// SPDX-License-Identifier: AGPL-3.0-only

use sqlx::{PgPool, Postgres, Row, Transaction};
use struxio_common::WorkspaceId;
use struxio_contracts::{
    CandidateEvaluation, EvalDecision, EvalThresholds, EvidenceReport, SchemaValidationReport,
    ValidationReport,
};
use uuid::Uuid;

use super::workspace_id_of;
use crate::codec::{decode_json, encode_json, enum_str, parse_decision};
use crate::error::ContractStoreError;
use crate::records::{
    EvalRunMetrics, NewEvalRunResult, NewValidationReport, StoredEvalRun, StoredEvalRunResult,
    StoredValidationReport,
};

pub struct ValidationRepo;
pub struct EvaluationRepo;

impl ValidationRepo {
    pub async fn insert(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        report: NewValidationReport,
    ) -> Result<StoredValidationReport, ContractStoreError> {
        let schema = encode_json("schema_report", report.evaluation.schema())?;
        let validation = encode_json("validation_report", report.evaluation.validation())?;
        let evidence = encode_json("evidence_report", report.evaluation.evidence())?;
        let row = sqlx::query(
            "INSERT INTO extraction_validation_reports \
             (workspace_id, extraction_id, contract_version_id, sidecar_id, normalized_data, \
              schema_report, validation_report, evidence_report, is_valid) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             RETURNING id, workspace_id, extraction_id, contract_version_id, sidecar_id, \
                       normalized_data, schema_report, validation_report, evidence_report, \
                       is_valid, created_at",
        )
        .bind(workspace_id.as_uuid())
        .bind(report.extraction_id)
        .bind(report.contract_version_id)
        .bind(report.sidecar_id)
        .bind(report.evaluation.normalized())
        .bind(schema)
        .bind(validation)
        .bind(evidence)
        .bind(report.evaluation.is_valid())
        .fetch_one(pool)
        .await?;
        row_to_validation(row).map_err(Into::into)
    }

    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<StoredValidationReport>, ContractStoreError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, extraction_id, contract_version_id, sidecar_id, \
                    normalized_data, schema_report, validation_report, evidence_report, \
                    is_valid, created_at \
             FROM extraction_validation_reports \
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;
        row.map(row_to_validation).transpose().map_err(Into::into)
    }

    pub async fn list_for_extraction(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        extraction_id: Uuid,
    ) -> Result<Vec<StoredValidationReport>, ContractStoreError> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, extraction_id, contract_version_id, sidecar_id, \
                    normalized_data, schema_report, validation_report, evidence_report, \
                    is_valid, created_at \
             FROM extraction_validation_reports \
             WHERE workspace_id = $1 AND extraction_id = $2 \
             ORDER BY created_at ASC",
        )
        .bind(workspace_id.as_uuid())
        .bind(extraction_id)
        .fetch_all(pool)
        .await?;
        rows.into_iter()
            .map(row_to_validation)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

impl EvaluationRepo {
    pub async fn insert_completed_run(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        contract_version_id: Uuid,
        metrics: EvalRunMetrics,
        results: &[NewEvalRunResult],
    ) -> Result<StoredEvalRun, ContractStoreError> {
        if results.is_empty() {
            return Err(ContractStoreError::invalid(
                "evaluation run must include at least one fixture result",
            ));
        }
        let mut tx = pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO extraction_eval_runs \
             (workspace_id, contract_version_id, schema_valid_rate_bps, field_accuracy_bps, \
              evidence_coverage_bps, abstain_rate_bps, passed) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, workspace_id, contract_version_id, schema_valid_rate_bps, \
                       field_accuracy_bps, evidence_coverage_bps, abstain_rate_bps, \
                       passed, created_at",
        )
        .bind(workspace_id.as_uuid())
        .bind(contract_version_id)
        .bind(metrics.schema_valid_rate_bps)
        .bind(metrics.field_accuracy_bps)
        .bind(metrics.evidence_coverage_bps)
        .bind(metrics.abstain_rate_bps)
        .bind(metrics.passed)
        .fetch_one(&mut *tx)
        .await?;
        let eval_run_id: Uuid = row.get("id");
        let mut stored_results = Vec::with_capacity(results.len());
        for result in results {
            stored_results
                .push(insert_result_in_tx(&mut tx, workspace_id, eval_run_id, result).await?);
        }
        tx.commit().await?;
        Ok(StoredEvalRun {
            id: eval_run_id,
            workspace_id: workspace_id_of(&row)?,
            contract_version_id: row.get("contract_version_id"),
            schema_valid_rate_bps: row.get("schema_valid_rate_bps"),
            field_accuracy_bps: row.get("field_accuracy_bps"),
            evidence_coverage_bps: row.get("evidence_coverage_bps"),
            abstain_rate_bps: row.get("abstain_rate_bps"),
            passed: row.get("passed"),
            created_at: row.get("created_at"),
            results: stored_results,
        })
    }

    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<StoredEvalRun>, ContractStoreError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, contract_version_id, schema_valid_rate_bps, \
                    field_accuracy_bps, evidence_coverage_bps, abstain_rate_bps, \
                    passed, created_at \
             FROM extraction_eval_runs \
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let results = list_results(pool, workspace_id, id).await?;
        Ok(Some(row_to_run(row, results)?))
    }

    pub async fn list_for_contract_version(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        contract_version_id: Uuid,
    ) -> Result<Vec<StoredEvalRun>, ContractStoreError> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, contract_version_id, schema_valid_rate_bps, \
                    field_accuracy_bps, evidence_coverage_bps, abstain_rate_bps, \
                    passed, created_at \
             FROM extraction_eval_runs \
             WHERE workspace_id = $1 AND contract_version_id = $2 \
             ORDER BY created_at ASC",
        )
        .bind(workspace_id.as_uuid())
        .bind(contract_version_id)
        .fetch_all(pool)
        .await?;
        let mut runs = Vec::with_capacity(rows.len());
        for row in rows {
            let id: Uuid = row.get("id");
            let results = list_results(pool, workspace_id, id).await?;
            runs.push(row_to_run(row, results)?);
        }
        Ok(runs)
    }
}

pub fn compute_eval_run_metrics(
    thresholds: EvalThresholds,
    results: &[NewEvalRunResult],
) -> EvalRunMetrics {
    let total = results.len();
    let schema_hits = results.iter().filter(|row| row.schema_valid).count();
    let field_hits = results.iter().filter(|row| row.fields_match).count();
    let evidence_hits = results.iter().filter(|row| row.evidence_satisfied).count();
    let abstain_hits = results
        .iter()
        .filter(|row| row.actual_decision == Some(EvalDecision::Abstain))
        .count();
    let schema_valid_rate_bps = rate_bps_floor(schema_hits, total);
    let field_accuracy_bps = rate_bps_floor(field_hits, total);
    let evidence_coverage_bps = rate_bps_floor(evidence_hits, total);
    let abstain_rate_bps = rate_bps_floor(abstain_hits, total);
    let passed = schema_valid_rate_bps >= i32::from(thresholds.min_schema_valid_rate_bps())
        && field_accuracy_bps >= i32::from(thresholds.min_field_accuracy_bps())
        && evidence_coverage_bps >= i32::from(thresholds.min_evidence_coverage_bps())
        && abstain_rate_bps <= i32::from(thresholds.max_abstain_rate_bps());
    EvalRunMetrics {
        schema_valid_rate_bps,
        field_accuracy_bps,
        evidence_coverage_bps,
        abstain_rate_bps,
        passed,
    }
}

fn rate_bps_floor(hits: usize, total: usize) -> i32 {
    if total == 0 {
        0
    } else {
        i32::try_from((hits as u128 * 10_000) / total as u128).unwrap_or(10_000)
    }
}

async fn insert_result_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: WorkspaceId,
    eval_run_id: Uuid,
    result: &NewEvalRunResult,
) -> Result<StoredEvalRunResult, ContractStoreError> {
    let decision = result.actual_decision.map(enum_str).transpose()?;
    let schema = encode_json("schema_report", &result.schema_report)?;
    let validation = encode_json("validation_report", &result.validation_report)?;
    let evidence = encode_json("evidence_report", &result.evidence_report)?;
    let row = sqlx::query(
        "INSERT INTO extraction_eval_run_results \
         (workspace_id, eval_run_id, fixture_id, sidecar_id, actual_data, actual_decision, \
          schema_valid, fields_match, evidence_satisfied, schema_report, validation_report, \
          evidence_report) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
         RETURNING id, workspace_id, eval_run_id, fixture_id, sidecar_id, actual_data, \
                   actual_decision, schema_valid, fields_match, evidence_satisfied, \
                   schema_report, validation_report, evidence_report, created_at",
    )
    .bind(workspace_id.as_uuid())
    .bind(eval_run_id)
    .bind(result.fixture_id)
    .bind(result.sidecar_id)
    .bind(&result.actual_data)
    .bind(decision)
    .bind(result.schema_valid)
    .bind(result.fields_match)
    .bind(result.evidence_satisfied)
    .bind(schema)
    .bind(validation)
    .bind(evidence)
    .fetch_one(&mut **tx)
    .await?;
    row_to_eval_result(row).map_err(Into::into)
}

async fn list_results(
    pool: &PgPool,
    workspace_id: WorkspaceId,
    eval_run_id: Uuid,
) -> Result<Vec<StoredEvalRunResult>, ContractStoreError> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, eval_run_id, fixture_id, sidecar_id, actual_data, \
                actual_decision, schema_valid, fields_match, evidence_satisfied, \
                schema_report, validation_report, evidence_report, created_at \
         FROM extraction_eval_run_results \
         WHERE workspace_id = $1 AND eval_run_id = $2 \
         ORDER BY created_at ASC",
    )
    .bind(workspace_id.as_uuid())
    .bind(eval_run_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(row_to_eval_result)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn row_to_validation(row: sqlx::postgres::PgRow) -> Result<StoredValidationReport, sqlx::Error> {
    let schema: SchemaValidationReport = decode_json("schema_report", row.get("schema_report"))?;
    let validation: ValidationReport =
        decode_json("validation_report", row.get("validation_report"))?;
    let evidence: EvidenceReport = decode_json("evidence_report", row.get("evidence_report"))?;
    let evaluation =
        CandidateEvaluation::from_parts(row.get("normalized_data"), schema, validation, evidence);
    let stored_valid: bool = row.get("is_valid");
    if stored_valid != evaluation.is_valid() {
        return Err(crate::error::decode_error(
            "is_valid",
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stored is_valid does not match reconstructed evaluation",
            ),
        ));
    }
    Ok(StoredValidationReport {
        id: row.get("id"),
        workspace_id: workspace_id_of(&row)?,
        extraction_id: row.get("extraction_id"),
        contract_version_id: row.get("contract_version_id"),
        sidecar_id: row.get("sidecar_id"),
        evaluation,
        created_at: row.get("created_at"),
    })
}

fn row_to_eval_result(row: sqlx::postgres::PgRow) -> Result<StoredEvalRunResult, sqlx::Error> {
    Ok(StoredEvalRunResult {
        id: row.get("id"),
        workspace_id: workspace_id_of(&row)?,
        eval_run_id: row.get("eval_run_id"),
        fixture_id: row.get("fixture_id"),
        sidecar_id: row.get("sidecar_id"),
        actual_data: row.get("actual_data"),
        actual_decision: parse_decision(row.get("actual_decision"))?,
        schema_valid: row.get("schema_valid"),
        fields_match: row.get("fields_match"),
        evidence_satisfied: row.get("evidence_satisfied"),
        schema_report: decode_json("schema_report", row.get("schema_report"))?,
        validation_report: decode_json("validation_report", row.get("validation_report"))?,
        evidence_report: decode_json("evidence_report", row.get("evidence_report"))?,
        created_at: row.get("created_at"),
    })
}

fn row_to_run(
    row: sqlx::postgres::PgRow,
    results: Vec<StoredEvalRunResult>,
) -> Result<StoredEvalRun, sqlx::Error> {
    Ok(StoredEvalRun {
        id: row.get("id"),
        workspace_id: workspace_id_of(&row)?,
        contract_version_id: row.get("contract_version_id"),
        schema_valid_rate_bps: row.get("schema_valid_rate_bps"),
        field_accuracy_bps: row.get("field_accuracy_bps"),
        evidence_coverage_bps: row.get("evidence_coverage_bps"),
        abstain_rate_bps: row.get("abstain_rate_bps"),
        passed: row.get("passed"),
        created_at: row.get("created_at"),
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn flags(
        schema_valid: bool,
        fields_match: bool,
        evidence_satisfied: bool,
        decision: Option<EvalDecision>,
    ) -> NewEvalRunResult {
        NewEvalRunResult {
            fixture_id: Uuid::nil(),
            sidecar_id: Uuid::nil(),
            actual_data: serde_json::json!({}),
            actual_decision: decision,
            schema_valid,
            fields_match,
            evidence_satisfied,
            schema_report: struxio_contracts::JsonSchema::new(
                serde_json::json!({"type": "object"}),
            )
            .unwrap()
            .validate(&serde_json::json!({})),
            validation_report: ValidationReport::from_failures(Vec::new()),
            evidence_report: struxio_contracts::check_policy(
                &serde_json::json!({}),
                &struxio_contracts::EvidenceSidecar::not_requested(),
                &struxio_contracts::EvidencePolicy::none(),
            ),
        }
    }

    #[test]
    fn floor_rates_are_fail_closed_against_strict_thresholds() {
        let metrics = compute_eval_run_metrics(
            EvalThresholds::strict(),
            &[
                flags(true, true, true, Some(EvalDecision::Accept)),
                flags(true, false, true, None),
            ],
        );
        assert_eq!(metrics.schema_valid_rate_bps, 10_000);
        assert_eq!(metrics.field_accuracy_bps, 5_000);
        assert!(!metrics.passed);
    }
}
