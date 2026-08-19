// SPDX-License-Identifier: AGPL-3.0-only

use sqlx::{PgPool, Postgres, Row, Transaction};
use struxio_common::WorkspaceId;
use struxio_contracts::{
    ExtractionContract, FixtureDescriptor, FixtureInput, PositiveVersion, Sha256ContentHash,
};
use uuid::Uuid;

use super::workspace_id_of;
use crate::codec::{
    decode_published_contract, encode_published_contract, enum_str, parse_decision, parse_hash,
};
use crate::error::{decode_error, ContractStoreError};
use crate::records::{StoredContractVersion, StoredFixture};
use crate::repositories::evidence::EvidenceRepo;

pub struct ContractRepo;

impl ContractRepo {
    pub async fn publish(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        contract: &ExtractionContract,
    ) -> Result<StoredContractVersion, ContractStoreError> {
        let encoded = encode_published_contract(contract)?;
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO extraction_contract_contents \
             (workspace_id, content_hash, canonical_payload, spec) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (workspace_id, content_hash) DO NOTHING",
        )
        .bind(workspace_id.as_uuid())
        .bind(encoded.hash.as_hex())
        .bind(&encoded.payload)
        .bind(&encoded.spec_json)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query(
            "INSERT INTO extraction_contract_versions \
             (workspace_id, slug, version, content_hash) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, workspace_id, slug, version, content_hash, created_at",
        )
        .bind(workspace_id.as_uuid())
        .bind(contract.identity().slug().as_str())
        .bind(i64::from(contract.identity().version().get()))
        .bind(encoded.hash.as_hex())
        .fetch_one(&mut *tx)
        .await?;

        let stored = row_to_version(&row, &encoded.payload)?;
        for fixture in contract.spec().fixtures() {
            let sidecar =
                EvidenceRepo::insert_in_tx(&mut tx, workspace_id, fixture.expected_evidence())
                    .await?;
            insert_fixture_in_tx(&mut tx, workspace_id, stored.id, fixture, sidecar.id).await?;
        }
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<StoredContractVersion>, ContractStoreError> {
        let row = sqlx::query(VERSION_SELECT)
            .bind(workspace_id.as_uuid())
            .bind(id)
            .fetch_optional(pool)
            .await?;
        row.map(row_to_version_joined)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn find_by_slug_version(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        slug: &str,
        version: PositiveVersion,
    ) -> Result<Option<StoredContractVersion>, ContractStoreError> {
        let row = sqlx::query(&format!(
            "{VERSION_SELECT_FROM} AND v.slug = $2 AND v.version = $3"
        ))
        .bind(workspace_id.as_uuid())
        .bind(slug)
        .bind(i64::from(version.get()))
        .fetch_optional(pool)
        .await?;
        row.map(row_to_version_joined)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn list_by_slug(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        slug: &str,
    ) -> Result<Vec<StoredContractVersion>, ContractStoreError> {
        let rows = sqlx::query(&format!(
            "{VERSION_SELECT_FROM} AND v.slug = $2 ORDER BY v.version ASC"
        ))
        .bind(workspace_id.as_uuid())
        .bind(slug)
        .fetch_all(pool)
        .await?;
        rows.into_iter()
            .map(row_to_version_joined)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub async fn find_by_content_hash(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        content_hash: Sha256ContentHash,
    ) -> Result<Vec<StoredContractVersion>, ContractStoreError> {
        let rows = sqlx::query(&format!(
            "{VERSION_SELECT_FROM} AND v.content_hash = $2 ORDER BY v.slug, v.version"
        ))
        .bind(workspace_id.as_uuid())
        .bind(content_hash.as_hex())
        .fetch_all(pool)
        .await?;
        rows.into_iter()
            .map(row_to_version_joined)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub async fn list_fixtures(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        contract_version_id: Uuid,
    ) -> Result<Vec<StoredFixture>, ContractStoreError> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, contract_version_id, name, input_file_name, \
                    input_media_type, input_content_sha256, expected_data, \
                    expected_sidecar_id, expected_decision, created_at \
             FROM extraction_contract_fixtures \
             WHERE workspace_id = $1 AND contract_version_id = $2 \
             ORDER BY name",
        )
        .bind(workspace_id.as_uuid())
        .bind(contract_version_id)
        .fetch_all(pool)
        .await?;

        let mut fixtures = Vec::with_capacity(rows.len());
        for row in rows {
            let sidecar_id: Uuid = row.get("expected_sidecar_id");
            let sidecar = EvidenceRepo::find_by_id(pool, workspace_id, sidecar_id)
                .await?
                .ok_or(ContractStoreError::NotFound("evidence sidecar"))?;
            fixtures.push(row_to_fixture(row, sidecar.sidecar)?);
        }
        Ok(fixtures)
    }
}

const VERSION_SELECT_FROM: &str =
    "SELECT v.id, v.workspace_id, v.slug, v.version, v.content_hash, \
     v.created_at, c.canonical_payload \
     FROM extraction_contract_versions v \
     JOIN extraction_contract_contents c \
       ON c.workspace_id = v.workspace_id AND c.content_hash = v.content_hash \
     WHERE v.workspace_id = $1";

const VERSION_SELECT: &str = "SELECT v.id, v.workspace_id, v.slug, v.version, v.content_hash, \
     v.created_at, c.canonical_payload \
     FROM extraction_contract_versions v \
     JOIN extraction_contract_contents c \
       ON c.workspace_id = v.workspace_id AND c.content_hash = v.content_hash \
     WHERE v.workspace_id = $1 AND v.id = $2";

fn row_to_version_joined(row: sqlx::postgres::PgRow) -> Result<StoredContractVersion, sqlx::Error> {
    let payload: Vec<u8> = row.get("canonical_payload");
    row_to_version(&row, &payload)
}

fn row_to_version(
    row: &sqlx::postgres::PgRow,
    payload: &[u8],
) -> Result<StoredContractVersion, sqlx::Error> {
    let slug: String = row.get("slug");
    let version: i64 = row.get("version");
    let hash: String = row.get("content_hash");
    let contract = decode_published_contract(&slug, version, &hash, payload)
        .map_err(|error| decode_error("canonical_payload", error))?;
    Ok(StoredContractVersion {
        id: row.get("id"),
        workspace_id: workspace_id_of(row)?,
        contract,
        created_at: row.get("created_at"),
    })
}

async fn insert_fixture_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: WorkspaceId,
    contract_version_id: Uuid,
    fixture: &FixtureDescriptor,
    expected_sidecar_id: Uuid,
) -> Result<StoredFixture, ContractStoreError> {
    let decision = fixture
        .expected_decision()
        .map(enum_str)
        .transpose()
        .map_err(ContractStoreError::from)?;
    let row = sqlx::query(
        "INSERT INTO extraction_contract_fixtures \
         (workspace_id, contract_version_id, name, input_file_name, input_media_type, \
          input_content_sha256, expected_data, expected_sidecar_id, expected_decision) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         RETURNING id, workspace_id, contract_version_id, name, input_file_name, \
                   input_media_type, input_content_sha256, expected_data, \
                   expected_sidecar_id, expected_decision, created_at",
    )
    .bind(workspace_id.as_uuid())
    .bind(contract_version_id)
    .bind(fixture.name())
    .bind(fixture.input().file_name())
    .bind(fixture.input().media_type())
    .bind(fixture.input().content_sha256().as_hex())
    .bind(fixture.expected_data())
    .bind(expected_sidecar_id)
    .bind(decision)
    .fetch_one(&mut **tx)
    .await?;
    row_to_fixture(row, fixture.expected_evidence().clone()).map_err(Into::into)
}

fn row_to_fixture(
    row: sqlx::postgres::PgRow,
    expected_evidence: struxio_contracts::EvidenceSidecar,
) -> Result<StoredFixture, sqlx::Error> {
    let input = FixtureInput::new(
        row.get::<String, _>("input_file_name"),
        row.get::<String, _>("input_media_type"),
        parse_hash(&row.get::<String, _>("input_content_sha256"))?,
    )
    .map_err(|error| decode_error("input_file_name", error))?;
    let descriptor = FixtureDescriptor::new(
        row.get::<String, _>("name"),
        input,
        row.get("expected_data"),
        expected_evidence,
        parse_decision(row.get("expected_decision"))?,
    )
    .map_err(|error| decode_error("name", error))?;
    Ok(StoredFixture {
        id: row.get("id"),
        workspace_id: workspace_id_of(&row)?,
        contract_version_id: row.get("contract_version_id"),
        descriptor,
        expected_sidecar_id: row.get("expected_sidecar_id"),
        created_at: row.get("created_at"),
    })
}
