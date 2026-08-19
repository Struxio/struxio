use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use struxio_common::WorkspaceId;
use struxio_contracts::{
    ContractIdentity, ContractSlug, ExtractionContract, PositiveVersion, Sha256ContentHash,
};
use uuid::Uuid;

use super::wave2::{content_hash_bytes, domain_error, from_json, hash_from_row, to_json};

#[derive(Debug, Clone)]
pub struct StoredContract {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub identity: ContractIdentity,
    pub contract: ExtractionContract,
    pub published_at: DateTime<Utc>,
}

pub struct ContractRepo;

const SELECT_COLS: &str =
    "id, workspace_id, slug, version, content_sha256, contract_json, published_at";

fn row_to_contract(row: sqlx::postgres::PgRow) -> Result<StoredContract, sqlx::Error> {
    let workspace_id = super::workspace_id_of(&row)?;
    let contract: ExtractionContract = from_json("contract_json", row.try_get("contract_json")?)?;
    let content_hash = hash_from_row(&row, "content_sha256")?;
    if contract.content_hash() != content_hash || !contract.verify_content_hash() {
        return Err(domain_error(
            "stored extraction contract failed its SHA-256 content identity check",
        ));
    }

    let slug = ContractSlug::new(row.try_get::<String, _>("slug")?)
        .map_err(|error| domain_error(format!("invalid stored contract slug: {error}")))?;
    let version = PositiveVersion::new(row.try_get::<i32, _>("version")? as u32)
        .map_err(|error| domain_error(format!("invalid stored contract version: {error}")))?;
    if contract.identity().slug() != &slug || contract.identity().version() != version {
        return Err(domain_error(
            "stored extraction contract columns do not match contract JSON",
        ));
    }

    Ok(StoredContract {
        id: row.try_get("id")?,
        workspace_id,
        identity: ContractIdentity::new(slug, version, content_hash),
        contract,
        published_at: row.try_get("published_at")?,
    })
}

impl ContractRepo {
    /// Publish one immutable contract version. A second publication of the
    /// same workspace/slug/version is rejected by the database.
    pub async fn publish(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        contract: &ExtractionContract,
    ) -> Result<StoredContract, sqlx::Error> {
        if !contract.verify_content_hash() {
            return Err(domain_error(
                "extraction contract failed its SHA-256 content identity check",
            ));
        }
        let contract_json = to_json("contract_json", contract)?;
        let row = sqlx::query(&format!(
            "INSERT INTO extraction_contracts \
             (workspace_id, slug, version, content_sha256, contract_json) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING {SELECT_COLS}"
        ))
        .bind(workspace_id.as_uuid())
        .bind(contract.identity().slug().as_str())
        .bind(contract.identity().version().get() as i32)
        .bind(content_hash_bytes(contract.content_hash()))
        .bind(contract_json)
        .fetch_one(pool)
        .await?;

        row_to_contract(row)
    }

    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<StoredContract>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extraction_contracts \
             WHERE workspace_id = $1 AND id = $2"
        ))
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.map(row_to_contract).transpose()
    }

    pub async fn find_by_identity(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        slug: &str,
        version: u32,
        content_hash: Sha256ContentHash,
    ) -> Result<Option<StoredContract>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLS} FROM extraction_contracts \
             WHERE workspace_id = $1 AND slug = $2 AND version = $3 AND content_sha256 = $4"
        ))
        .bind(workspace_id.as_uuid())
        .bind(slug)
        .bind(version as i32)
        .bind(content_hash_bytes(content_hash))
        .fetch_optional(pool)
        .await?;

        row.map(row_to_contract).transpose()
    }

    pub async fn list_published(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        slug: Option<&str>,
    ) -> Result<Vec<StoredContract>, sqlx::Error> {
        let rows = match slug {
            Some(slug) => {
                sqlx::query(&format!(
                    "SELECT {SELECT_COLS} FROM extraction_contracts \
                     WHERE workspace_id = $1 AND slug = $2 \
                     ORDER BY slug, version DESC"
                ))
                .bind(workspace_id.as_uuid())
                .bind(slug)
                .fetch_all(pool)
                .await?
            }
            None => {
                sqlx::query(&format!(
                    "SELECT {SELECT_COLS} FROM extraction_contracts \
                     WHERE workspace_id = $1 ORDER BY slug, version DESC"
                ))
                .bind(workspace_id.as_uuid())
                .fetch_all(pool)
                .await?
            }
        };

        rows.into_iter().map(row_to_contract).collect()
    }
}
