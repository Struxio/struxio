use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use struxio_common::WorkspaceId;
use struxio_contracts::{
    CandidateEvaluation, SchemaValidationReport, Sha256ContentHash, ValidationReport,
};
use uuid::Uuid;

use super::wave2::{content_hash_bytes, from_json, hash_from_row, hash_json, to_json};

/// The persisted validation payload intentionally excludes normalized result
/// data and evidence. Both are independently addressable records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReportPayload {
    pub schema: SchemaValidationReport,
    pub validation: ValidationReport,
}

impl ValidationReportPayload {
    pub fn from_candidate(candidate: &CandidateEvaluation) -> Self {
        Self {
            schema: candidate.schema().clone(),
            validation: candidate.validation().clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredValidationReport {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub extraction_id: Option<Uuid>,
    pub evaluation_run_result_id: Option<Uuid>,
    pub contract_id: Uuid,
    pub contract_content_sha256: Sha256ContentHash,
    pub report: ValidationReportPayload,
    pub report_sha256: Sha256ContentHash,
    pub created_at: DateTime<Utc>,
}

pub struct ValidationReportRepo;

const SELECT_COLS: &str = "id, workspace_id, extraction_id, evaluation_run_result_id, \
    contract_id, contract_content_sha256, report_json, report_sha256, created_at";

fn row_to_report(row: sqlx::postgres::PgRow) -> Result<StoredValidationReport, sqlx::Error> {
    let report_json = row.try_get("report_json")?;
    let report: ValidationReportPayload = from_json("report_json", report_json.clone())?;
    let report_sha256 = hash_from_row(&row, "report_sha256")?;
    if hash_json(&report_json) != report_sha256 {
        return Err(super::wave2::domain_error(
            "stored validation report failed its SHA-256 content identity check",
        ));
    }

    Ok(StoredValidationReport {
        id: row.try_get("id")?,
        workspace_id: super::workspace_id_of(&row)?,
        extraction_id: row.try_get("extraction_id")?,
        evaluation_run_result_id: row.try_get("evaluation_run_result_id")?,
        contract_id: row.try_get("contract_id")?,
        contract_content_sha256: hash_from_row(&row, "contract_content_sha256")?,
        report,
        report_sha256,
        created_at: row.try_get("created_at")?,
    })
}

impl ValidationReportRepo {
    pub async fn append_for_extraction(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        extraction_id: Uuid,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
        report: &ValidationReportPayload,
    ) -> Result<StoredValidationReport, sqlx::Error> {
        Self::append(
            pool,
            workspace_id,
            Some(extraction_id),
            None,
            contract_id,
            contract_content_sha256,
            report,
        )
        .await
    }

    pub async fn append_for_evaluation_result(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        evaluation_run_result_id: Uuid,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
        report: &ValidationReportPayload,
    ) -> Result<StoredValidationReport, sqlx::Error> {
        Self::append(
            pool,
            workspace_id,
            None,
            Some(evaluation_run_result_id),
            contract_id,
            contract_content_sha256,
            report,
        )
        .await
    }

    async fn append(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        extraction_id: Option<Uuid>,
        evaluation_run_result_id: Option<Uuid>,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
        report: &ValidationReportPayload,
    ) -> Result<StoredValidationReport, sqlx::Error> {
        let report_json = to_json("report_json", report)?;
        let report_sha256 = hash_json(&report_json);
        let row = sqlx::query(&format!(
            "INSERT INTO validation_reports \
             (workspace_id, extraction_id, evaluation_run_result_id, contract_id, \
              contract_content_sha256, report_json, report_sha256) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING {SELECT_COLS}"
        ))
        .bind(workspace_id.as_uuid())
        .bind(extraction_id)
        .bind(evaluation_run_result_id)
        .bind(contract_id)
        .bind(content_hash_bytes(contract_content_sha256))
        .bind(report_json)
        .bind(content_hash_bytes(report_sha256))
        .fetch_one(pool)
        .await?;

        row_to_report(row)
    }

    pub async fn find_for_extraction(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        extraction_id: Uuid,
    ) -> Result<Option<StoredValidationReport>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM validation_reports \
             WHERE workspace_id = $1 AND extraction_id = $2"
        ))
        .bind(workspace_id.as_uuid())
        .bind(extraction_id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_report).transpose()
    }

    pub async fn find_for_evaluation_result(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        evaluation_run_result_id: Uuid,
    ) -> Result<Option<StoredValidationReport>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM validation_reports \
             WHERE workspace_id = $1 AND evaluation_run_result_id = $2"
        ))
        .bind(workspace_id.as_uuid())
        .bind(evaluation_run_result_id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_report).transpose()
    }
}
