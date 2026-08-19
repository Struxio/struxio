// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::scopes::{Scope, ScopeSet};
use crate::AppError;

/// UUID v5 (DNS namespace + `local.struxio`). Must stay in sync with
/// `migrations/0005_workspaces_principals.sql`.
pub const LOCAL_WORKSPACE_UUID: Uuid = uuid::uuid!("79ca2631-6230-5277-9d0e-879be839f744");

/// UUID v5 (DNS namespace + `local.struxio.operator`). Must stay in sync with
/// `migrations/0005_workspaces_principals.sql`.
pub const LOCAL_PRINCIPAL_UUID: Uuid = uuid::uuid!("271a2afd-e58f-5f5b-9c32-57c0f38df33e");

/// Slug of the seeded single-tenant OSS workspace.
pub const LOCAL_WORKSPACE_SLUG: &str = "local";

/// Non-nil workspace identifier. Construction from `Uuid::nil()` is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(Uuid);

impl WorkspaceId {
    pub fn new(id: Uuid) -> Result<Self, AppError> {
        if id.is_nil() {
            return Err(AppError::Internal(
                "workspace_id must not be the nil UUID".to_string(),
            ));
        }
        Ok(Self(id))
    }

    pub fn local() -> Self {
        Self(LOCAL_WORKSPACE_UUID)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl TryFrom<Uuid> for WorkspaceId {
    type Error = AppError;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<WorkspaceId> for Uuid {
    fn from(value: WorkspaceId) -> Self {
        value.0
    }
}

impl std::fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Non-nil principal identifier. Construction from `Uuid::nil()` is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrincipalId(Uuid);

impl PrincipalId {
    pub fn new(id: Uuid) -> Result<Self, AppError> {
        if id.is_nil() {
            return Err(AppError::Internal(
                "principal_id must not be the nil UUID".to_string(),
            ));
        }
        Ok(Self(id))
    }

    pub fn local() -> Self {
        Self(LOCAL_PRINCIPAL_UUID)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl TryFrom<Uuid> for PrincipalId {
    type Error = AppError;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PrincipalId> for Uuid {
    fn from(value: PrincipalId) -> Self {
        value.0
    }
}

impl std::fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Authenticated caller bound to a real workspace. Cannot be constructed
/// without a non-nil [`WorkspaceId`].
#[derive(Debug, Clone)]
pub struct PrincipalContext {
    workspace_id: WorkspaceId,
    principal_id: PrincipalId,
    scopes: ScopeSet,
}

impl PrincipalContext {
    pub fn new(workspace_id: WorkspaceId, principal_id: PrincipalId, scopes: ScopeSet) -> Self {
        Self {
            workspace_id,
            principal_id,
            scopes,
        }
    }

    /// Local OSS operator: seeded workspace, seeded principal, all OSS scopes.
    pub fn local_operator() -> Self {
        Self::new(WorkspaceId::local(), PrincipalId::local(), ScopeSet::all())
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }

    pub fn scopes(&self) -> &ScopeSet {
        &self.scopes
    }

    pub fn has_scope(&self, scope: Scope) -> bool {
        self.scopes.contains(scope)
    }

    pub fn require_scope(&self, scope: Scope) -> Result<(), AppError> {
        self.scopes.require(scope)
    }
}

/// Worker-side counterpart to [`PrincipalContext`]. Carries a real workspace
/// without implying an interactive principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobExecutionContext {
    workspace_id: WorkspaceId,
}

impl JobExecutionContext {
    pub fn new(workspace_id: WorkspaceId) -> Self {
        Self { workspace_id }
    }

    pub fn workspace_id(self) -> WorkspaceId {
        self.workspace_id
    }
}

impl From<&PrincipalContext> for JobExecutionContext {
    fn from(ctx: &PrincipalContext) -> Self {
        Self::new(ctx.workspace_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_id_rejects_nil() {
        let err = WorkspaceId::new(Uuid::nil()).unwrap_err();
        match err {
            AppError::Internal(msg) => assert!(msg.contains("nil")),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn principal_id_rejects_nil() {
        assert!(PrincipalId::new(Uuid::nil()).is_err());
    }

    #[test]
    fn local_ids_are_not_nil() {
        assert!(!LOCAL_WORKSPACE_UUID.is_nil());
        assert!(!LOCAL_PRINCIPAL_UUID.is_nil());
        assert_ne!(LOCAL_WORKSPACE_UUID, LOCAL_PRINCIPAL_UUID);
        assert_eq!(WorkspaceId::local().as_uuid(), LOCAL_WORKSPACE_UUID);
        assert_eq!(PrincipalId::local().as_uuid(), LOCAL_PRINCIPAL_UUID);
    }

    #[test]
    fn principal_context_cannot_omit_workspace() {
        let ctx = PrincipalContext::local_operator();
        assert!(!ctx.workspace_id().as_uuid().is_nil());
        assert!(ctx.has_scope(Scope::DocumentsRead));
        assert!(ctx.has_scope(Scope::BatchesCreate));
    }

    #[test]
    fn job_execution_context_carries_real_workspace() {
        let ctx = JobExecutionContext::new(WorkspaceId::local());
        assert!(!ctx.workspace_id().as_uuid().is_nil());
    }
}
