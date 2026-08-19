use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use struxio_common::WorkspaceId;
use struxio_contracts::{EvidenceSidecar, Sha256ContentHash};
use uuid::Uuid;

use super::wave2::{content_hash_bytes, from_json, hash_from_row, hash_json, to_json};

#[derive(Debug, Clone)]
pub struct StoredEvidenceSidecar {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub extraction_id: Uuid,
    pub contract_id: Uuid,
    pub contract_content_sha256: Sha256ContentHash,
    pub sidecar: EvidenceSidecar,
    pub sidecar_sha256: Sha256ContentHash,
    pub created_at: DateTime<Utc>,
}

pub struct EvidenceSidecarRepo;

const SELECT_COLS: &str = "id, workspace_id, extraction_id, contract_id, \
    contract_content_sha256, sidecar_json, sidecar_sha256, created_at";

fn row_to_sidecar(row: sqlx::postgres::PgRow) -> Result<StoredEvidenceSidecar, sqlx::Error> {
    let sidecar_json: Value = row.try_get("sidecar_json")?;
    let sidecar: EvidenceSidecar = from_json("sidecar_json", sidecar_json.clone())?;
    let sidecar_sha256 = hash_from_row(&row, "sidecar_sha256")?;
    if hash_json(&sidecar_json) != sidecar_sha256 {
        return Err(super::wave2::domain_error(
            "stored evidence sidecar failed its SHA-256 content identity check",
        ));
    }

    Ok(StoredEvidenceSidecar {
        id: row.try_get("id")?,
        workspace_id: super::workspace_id_of(&row)?,
        extraction_id: row.try_get("extraction_id")?,
        contract_id: row.try_get("contract_id")?,
        contract_content_sha256: hash_from_row(&row, "contract_content_sha256")?,
        sidecar,
        sidecar_sha256,
        created_at: row.try_get("created_at")?,
    })
}

impl EvidenceSidecarRepo {
    /// Append the one immutable sidecar associated with an extraction result.
    /// The database uniqueness constraint prevents replacement or a second
    /// competing sidecar for the same extraction.
    pub async fn append(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        extraction_id: Uuid,
        contract_id: Uuid,
        contract_content_sha256: Sha256ContentHash,
        sidecar: &EvidenceSidecar,
    ) -> Result<StoredEvidenceSidecar, sqlx::Error> {
        let sidecar_json = to_json("sidecar_json", sidecar)?;
        let sidecar_sha256 = hash_json(&sidecar_json);
        let row = sqlx::query(&format!(
            "INSERT INTO extraction_evidence_sidecars \
             (workspace_id, extraction_id, contract_id, contract_content_sha256, \
              sidecar_json, sidecar_sha256) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING {SELECT_COLS}"
        ))
        .bind(workspace_id.as_uuid())
        .bind(extraction_id)
        .bind(contract_id)
        .bind(content_hash_bytes(contract_content_sha256))
        .bind(sidecar_json)
        .bind(content_hash_bytes(sidecar_sha256))
        .fetch_one(pool)
        .await?;

        row_to_sidecar(row)
    }

    pub async fn find_for_extraction(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        extraction_id: Uuid,
    ) -> Result<Option<StoredEvidenceSidecar>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extraction_evidence_sidecars \
             WHERE workspace_id = $1 AND extraction_id = $2"
        ))
        .bind(workspace_id.as_uuid())
        .bind(extraction_id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_sidecar).transpose()
    }
}
