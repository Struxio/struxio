// SPDX-License-Identifier: AGPL-3.0-only

use sqlx::{PgPool, Postgres, Row, Transaction};
use struxio_common::WorkspaceId;
use struxio_contracts::{EvidenceEntry, EvidenceKind, EvidenceSidecar, JsonPointer};
use uuid::Uuid;

use super::workspace_id_of;
use crate::codec::{
    enum_str, group_evidence_entries, parse_enum, parse_sidecar_status, StoredEvidenceRow,
};
use crate::error::ContractStoreError;
use crate::records::{StoredEvidenceAttachment, StoredEvidenceSidecar};

pub struct EvidenceRepo;

impl EvidenceRepo {
    pub async fn insert(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        sidecar: &EvidenceSidecar,
    ) -> Result<StoredEvidenceSidecar, ContractStoreError> {
        let mut tx = pool.begin().await?;
        let stored = Self::insert_in_tx(&mut tx, workspace_id, sidecar).await?;
        tx.commit().await?;
        Ok(stored)
    }

    pub(crate) async fn insert_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        workspace_id: WorkspaceId,
        sidecar: &EvidenceSidecar,
    ) -> Result<StoredEvidenceSidecar, ContractStoreError> {
        let status = enum_str(sidecar.status())?;
        let row = sqlx::query(
            "INSERT INTO extraction_evidence_sidecars \
             (workspace_id, sidecar_version, status) \
             VALUES ($1, $2, $3) \
             RETURNING id, workspace_id, sidecar_version, status, created_at",
        )
        .bind(workspace_id.as_uuid())
        .bind(i32::from(sidecar.version()))
        .bind(status)
        .fetch_one(&mut **tx)
        .await?;
        let sidecar_id: Uuid = row.get("id");

        for (pointer, entries) in sidecar.entries() {
            for (ordinal, entry) in entries.iter().enumerate() {
                let ordinal = i32::try_from(ordinal).map_err(|_| {
                    ContractStoreError::invalid("evidence ordinal does not fit in integer")
                })?;
                sqlx::query(
                    "INSERT INTO extraction_evidence_entries \
                     (workspace_id, sidecar_id, json_pointer, ordinal, kind, quote, source) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7)",
                )
                .bind(workspace_id.as_uuid())
                .bind(sidecar_id)
                .bind(pointer.to_string())
                .bind(ordinal)
                .bind(enum_str(entry.kind())?)
                .bind(entry.quote())
                .bind(entry.source())
                .execute(&mut **tx)
                .await?;
            }
        }

        Ok(StoredEvidenceSidecar {
            id: sidecar_id,
            workspace_id: workspace_id_of(&row)?,
            sidecar: sidecar.clone(),
            created_at: row.get("created_at"),
        })
    }

    pub async fn find_by_id(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        id: Uuid,
    ) -> Result<Option<StoredEvidenceSidecar>, ContractStoreError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, sidecar_version, status, created_at \
             FROM extraction_evidence_sidecars \
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let entries = load_entries(pool, workspace_id, id).await?;
        Ok(Some(row_to_sidecar(row, entries)?))
    }

    pub async fn entries_for_pointer(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        sidecar_id: Uuid,
        pointer: &JsonPointer,
    ) -> Result<Vec<EvidenceEntry>, ContractStoreError> {
        let exists = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM extraction_evidence_sidecars \
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id.as_uuid())
        .bind(sidecar_id)
        .fetch_optional(pool)
        .await?;
        if exists.is_none() {
            return Err(ContractStoreError::NotFound("evidence sidecar"));
        }

        let rows = sqlx::query(
            "SELECT kind, quote, source \
             FROM extraction_evidence_entries \
             WHERE workspace_id = $1 AND sidecar_id = $2 AND json_pointer = $3 \
             ORDER BY ordinal",
        )
        .bind(workspace_id.as_uuid())
        .bind(sidecar_id)
        .bind(pointer.to_string())
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let kind: EvidenceKind = parse_enum("kind", &row.get::<String, _>("kind"))?;
                EvidenceEntry::new(kind, row.get("quote"), row.get("source"))
                    .map_err(|error| crate::error::decode_error("quote", error))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub async fn attach_to_extraction(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        extraction_id: Uuid,
        sidecar_id: Uuid,
    ) -> Result<StoredEvidenceAttachment, ContractStoreError> {
        let row = sqlx::query(
            "INSERT INTO extraction_evidence_attachments \
             (workspace_id, extraction_id, sidecar_id) \
             VALUES ($1, $2, $3) \
             RETURNING id, workspace_id, extraction_id, sidecar_id, created_at",
        )
        .bind(workspace_id.as_uuid())
        .bind(extraction_id)
        .bind(sidecar_id)
        .fetch_one(pool)
        .await?;
        Ok(StoredEvidenceAttachment {
            id: row.get("id"),
            workspace_id: workspace_id_of(&row)?,
            extraction_id: row.get("extraction_id"),
            sidecar_id: row.get("sidecar_id"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn list_for_extraction(
        pool: &PgPool,
        workspace_id: WorkspaceId,
        extraction_id: Uuid,
    ) -> Result<Vec<StoredEvidenceSidecar>, ContractStoreError> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT sidecar_id FROM extraction_evidence_attachments \
             WHERE workspace_id = $1 AND extraction_id = $2 \
             ORDER BY created_at ASC",
        )
        .bind(workspace_id.as_uuid())
        .bind(extraction_id)
        .fetch_all(pool)
        .await?;
        let mut sidecars = Vec::with_capacity(ids.len());
        for id in ids {
            let sidecar = Self::find_by_id(pool, workspace_id, id)
                .await?
                .ok_or(ContractStoreError::NotFound("evidence sidecar"))?;
            sidecars.push(sidecar);
        }
        Ok(sidecars)
    }
}

async fn load_entries(
    pool: &PgPool,
    workspace_id: WorkspaceId,
    sidecar_id: Uuid,
) -> Result<Vec<StoredEvidenceRow>, ContractStoreError> {
    let rows = sqlx::query(
        "SELECT json_pointer, ordinal, kind, quote, source \
         FROM extraction_evidence_entries \
         WHERE workspace_id = $1 AND sidecar_id = $2 \
         ORDER BY json_pointer, ordinal",
    )
    .bind(workspace_id.as_uuid())
    .bind(sidecar_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let kind: EvidenceKind = parse_enum("kind", &row.get::<String, _>("kind"))?;
            Ok((
                row.get("json_pointer"),
                row.get("ordinal"),
                kind,
                row.get("quote"),
                row.get("source"),
            ))
        })
        .collect()
}

fn row_to_sidecar(
    row: sqlx::postgres::PgRow,
    entry_rows: Vec<StoredEvidenceRow>,
) -> Result<StoredEvidenceSidecar, sqlx::Error> {
    let version: i32 = row.get("sidecar_version");
    let version = u16::try_from(version)
        .map_err(|error| crate::error::decode_error("sidecar_version", error))?;
    let status = parse_sidecar_status(&row.get::<String, _>("status"))?;
    let entries = group_evidence_entries(entry_rows)?;
    Ok(StoredEvidenceSidecar {
        id: row.get("id"),
        workspace_id: workspace_id_of(&row)?,
        sidecar: EvidenceSidecar::from_stored(version, status, entries),
        created_at: row.get("created_at"),
    })
}
