use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use struxio_common::WorkspaceId;
use struxio_contracts::{
    CandidateEvaluation, EvalDecision, EvalThresholds, EvidenceSidecar, FixtureDescriptor,
    FixtureInput, Sha256ContentHash,
};
use uuid::Uuid;

use super::wave2::{
    content_hash_bytes, decision_text, domain_error, from_json, hash_from_row, optional_decision,
    to_json,
};

#[derive(Debug, Clone)]
pub struct StoredEvaluationFixture {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub contract_id: Uuid,
    pub contract_content_sha256: Sha256ContentHash,
    pub fixture: FixtureDescriptor,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredEvaluationRun {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub contract_id: Uuid,
    pub contract_content_sha256: Sha256ContentHash,
    pub policy: EvalThresholds,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredEvaluationRunResult {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub evaluation_run_id: Uuid,
    pub fixture_id: Uuid,
    pub contract_id: Uuid,
    pub contract_content_sha256: Sha256ContentHash,
    /// This is the schema-pure extracted JSON. Evidence is stored separately.
    pub result: Value,
    pub evidence: EvidenceSidecar,
    pub decision: Option<EvalDecision>,
    pub created_at: DateTime<Utc>,
}

pub struct EvaluationFixtureRepo;
pub struct EvaluationRunRepo;

const FIXTURE_COLS: &str = "id, workspace_id, contract_id, contract_content_sha256, \
    name, input_file_name, input_media_type, input_content_sha256, expected_result, \
    expected_evidence, expected_decision, created_at";
const RUN_COLS: &str =
    "id, workspace_id, contract_id, contract_content_sha256, policy_json, created_at";
const RESULT_COLS: &str = "id, workspace_id, evaluation_run_id, fixture_id, contract_id, \
    contract_content_sha256, result_json, evidence_json, decision, created_at";

fn row_to_fixture(row: sqlx::postgres::PgRow) -> Result<StoredEvaluationFixture, sqlx::Error> {
    let input_hash = hash_from_row(&row, "input_content_sha256")?;
    let input = FixtureInput::new(
        row.try_get::<String, _>("input_file_name")?,
        row.try_get::<String, _>("input_media_type")?,
        input_hash,
    )
    .map_err(|error| domain_error(format!("invalid stored fixture input: {error}")))?;
    let expected_evidence: EvidenceSidecar =
        from_json("expected_evidence", row.try_get("expected_evidence")?)?;
    let fixture = FixtureDescriptor::new(
        row.try_get::<String, _>("name")?,
        input,
        row.try_get("expected_result")?,
        expected_evidence,
        optional_decision(row.try_get("expected_decision")?)?,
    )
    .map_err(|error| domain_error(format!("invalid stored fixture: {error}")))?;

    Ok(StoredEvaluationFixture {
        id: row.try_get("id")?,
        workspace_id: super::workspace_id_of(&row)?,
        contract_id: row.try_get("contract_id")?,
        contract_content_sha256: hash_from_row(&row, "contract_content_sha256")?,
        fixture,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_run(row: sqlx::postgres::PgRow) -> Result<StoredEvaluationRun, sqlx::Error> {
    Ok(StoredEvaluationRun {
        id: row.try_get("id")?,
        workspace_id: super::workspace_id_of(&row)?,
        contract_id: row.try_get("contract_id")?,
        contract_content_sha256: hash_from_row(&row, "contract_content_sha256")?,
        policy: from_json("policy_json", row.try_get("policy_json")?)?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_result(row: sqlx::postgres::PgRow) -> Result<StoredEvaluationRunResult, sqlx::Error> {
    Ok(StoredEvaluationRunResult {
        id: row.try_get("id")?,
        workspace_id: super::workspace_id_of(&row)?,
        evaluation_run_id: row.try_get("evaluation_run_id")?,
        fixture_id: row.try_get("fixture_id")?,
        contract_id: row.try_get("contract_id")?,
        contract_content_sha256: hash_from_row(&row, "contract_content_sha256")?,
        result: row.try_get("result_json")?,
        evidence: from_json("evidence_json", row.try_get("evidence_json")?)?,
        decision: optional_decision(row.try_get("decision")?)?,
        created_at: row.try_get("created_at")?,
    })
}

impl EvaluationFixtureRepo {
    pub async fn append(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
        fixture: &FixtureDescriptor,
    ) -> Result<StoredEvaluationFixture, sqlx::Error> {
        let expected_result = to_json("expected_result", fixture.expected_data())?;
        let expected_evidence = to_json("expected_evidence", fixture.expected_evidence())?;
        let row = sqlx::query(&format!(
            "INSERT INTO evaluation_fixtures \
             (workspace_id, contract_id, contract_content_sha256, name, input_file_name, \
              input_media_type, input_content_sha256, expected_result, expected_evidence, \
              expected_decision) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             RETURNING {FIXTURE_COLS}"
        ))
        .bind(workspace_id.as_uuid())
        .bind(contract_id)
        .bind(content_hash_bytes(contract_content_sha256))
        .bind(fixture.name())
        .bind(fixture.input().file_name())
        .bind(fixture.input().media_type())
        .bind(content_hash_bytes(fixture.input().content_sha256()))
        .bind(expected_result)
        .bind(expected_evidence)
        .bind(decision_text(fixture.expected_decision()))
        .fetch_one(pool)
        .await?;

        row_to_fixture(row)
    }

    pub async fn list_for_contract(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
    ) -> Result<Vec<StoredEvaluationFixture>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {FIXTURE_COLS} FROM evaluation_fixtures \
             WHERE workspace_id = $1 AND contract_id = $2 AND contract_content_sha256 = $3 \
             ORDER BY name"
        ))
        .bind(workspace_id.as_uuid())
        .bind(contract_id)
        .bind(content_hash_bytes(contract_content_sha256))
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(row_to_fixture).collect()
    }
}

impl EvaluationRunRepo {
    pub async fn start(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
        policy: EvalThresholds,
    ) -> Result<StoredEvaluationRun, sqlx::Error> {
        let policy_json = to_json("policy_json", &policy)?;
        let row = sqlx::query(&format!(
            "INSERT INTO evaluation_runs \
             (workspace_id, contract_id, contract_content_sha256, policy_json) \
             VALUES ($1, $2, $3, $4) \
             RETURNING {RUN_COLS}"
        ))
        .bind(workspace_id.as_uuid())
        .bind(contract_id)
        .bind(content_hash_bytes(contract_content_sha256))
        .bind(policy_json)
        .fetch_one(pool)
        .await?;

        row_to_run(row)
    }

    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<StoredEvaluationRun>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {RUN_COLS} FROM evaluation_runs \
             WHERE workspace_id = $1 AND id = $2"
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_run).transpose()
    }

    pub async fn append_result(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        evaluation_run_id: Uuid,
        fixture_id: Uuid,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
        result: &Value,
        evidence: &EvidenceSidecar,
        decision: Option<EvalDecision>,
    ) -> Result<StoredEvaluationRunResult, sqlx::Error> {
        let evidence_json = to_json("evidence_json", evidence)?;
        let row = sqlx::query(&format!(
            "INSERT INTO evaluation_run_results \
             (workspace_id, evaluation_run_id, fixture_id, contract_id, \
              contract_content_sha256, result_json, evidence_json, decision) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING {RESULT_COLS}"
        ))
        .bind(workspace_id.as_uuid())
        .bind(evaluation_run_id)
        .bind(fixture_id)
        .bind(contract_id)
        .bind(content_hash_bytes(contract_content_sha256))
        .bind(result)
        .bind(evidence_json)
        .bind(decision_text(decision))
        .fetch_one(pool)
        .await?;

        row_to_result(row)
    }

    /// Convenience mapping for an evaluated candidate. The normalized value
    /// remains the schema-pure result; the candidate's evidence report is
    /// persisted later through ValidationReportRepo without wrapping either
    /// into the result JSON.
    pub async fn append_candidate_result(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        evaluation_run_id: Uuid,
        fixture_id: Uuid,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
        candidate: &CandidateEvaluation,
        evidence: &EvidenceSidecar,
        decision: Option<EvalDecision>,
    ) -> Result<StoredEvaluationRunResult, sqlx::Error> {
        Self::append_result(
            pool,
            workspace_id,
            evaluation_run_id,
            fixture_id,
            contract_id,
            contract_content_sha256,
            candidate.normalized(),
            evidence,
            decision,
        )
        .await
    }

    pub async fn list_results(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        evaluation_run_id: Uuid,
    ) -> Result<Vec<StoredEvaluationRunResult>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {RESULT_COLS} FROM evaluation_run_results \
             WHERE workspace_id = $1 AND evaluation_run_id = $2 ORDER BY created_at, id"
        ))
        .bind(workspace_id.as_uuid())
        .bind(evaluation_run_id)
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(row_to_result).collect()
    }
}
