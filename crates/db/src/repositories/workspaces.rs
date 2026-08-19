// SPDX-License-Identifier: AGPL-3.0-only

use sqlx::{PgPool, Row};
use struxio_common::{AppError, PrincipalContext, PrincipalId, WorkspaceId, LOCAL_WORKSPACE_SLUG};

fn decode_id<E: std::error::Error + Send + Sync + 'static>(column: &str, err: E) -> sqlx::Error {
    sqlx::Error::ColumnDecode {
        index: column.into(),
        source: Box::new(err),
    }
}

pub(crate) fn workspace_id_of(row: &sqlx::postgres::PgRow) -> Result<WorkspaceId, sqlx::Error> {
    let id: uuid::Uuid = row.get("workspace_id");
    WorkspaceId::new(id).map_err(|e| decode_id("workspace_id", e))
}

/// Loads the seeded local OSS workspace and operator principal from Postgres.
pub struct WorkspaceRepo;

impl WorkspaceRepo {
    pub async fn load_local_principal(pool: &PgPool) -> Result<PrincipalContext, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT w.id AS workspace_id, p.id AS principal_id, m.scopes
            FROM workspaces w
            JOIN workspace_memberships m ON m.workspace_id = w.id
            JOIN principals p ON p.id = m.principal_id
            WHERE w.slug = $1
              AND p.provider = 'local'
              AND p.subject = 'operator'
            "#,
        )
        .bind(LOCAL_WORKSPACE_SLUG)
        .fetch_one(pool)
        .await?;

        let workspace_id =
            WorkspaceId::new(row.get("workspace_id")).map_err(|e| decode_id("workspace_id", e))?;
        let principal_id =
            PrincipalId::new(row.get("principal_id")).map_err(|e| decode_id("principal_id", e))?;
        let scopes: Vec<String> = row.get("scopes");
        Ok(PrincipalContext::new(
            workspace_id,
            principal_id,
            struxio_common::ScopeSet::parse(scopes),
        ))
    }

    pub async fn load_local_principal_or_err(pool: &PgPool) -> Result<PrincipalContext, AppError> {
        Self::load_local_principal(pool).await.map_err(|e| {
            AppError::Internal(format!("failed to load local workspace principal: {e}"))
        })
    }
}
