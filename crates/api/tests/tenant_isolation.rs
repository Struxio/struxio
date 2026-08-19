// SPDX-License-Identifier: AGPL-3.0-only

//! Tenant isolation coverage for scoped repositories and principal scopes.
//! Requires `DATABASE_URL` for the repository cases. Not executed as part of
//! this change.

use sqlx::PgPool;
use uuid::Uuid;

use struxio_common::{
    PrincipalContext, PrincipalId, Scope, ScopeSet, WorkspaceId, LOCAL_WORKSPACE_UUID,
};
use struxio_db::repositories::documents::DocumentRepo;
use struxio_db::repositories::templates::TemplateRepo;

async fn connect() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let pool = struxio_db::create_pool(&url)
        .await
        .expect("connect to postgres");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

fn principal_for(workspace: WorkspaceId, scopes: impl IntoIterator<Item = Scope>) -> PrincipalContext {
    PrincipalContext::new(workspace, PrincipalId::local(), ScopeSet::from_scopes(scopes))
}

#[test]
fn workspace_id_rejects_nil_uuid() {
    assert!(WorkspaceId::new(Uuid::nil()).is_err());
    assert!(!LOCAL_WORKSPACE_UUID.is_nil());
}

#[test]
fn missing_scope_is_forbidden_without_resource_leak() {
    let ctx = principal_for(WorkspaceId::local(), [Scope::DocumentsRead]);
    let err = ctx.require_scope(Scope::DocumentsWrite).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("insufficient scope"));
    assert!(!msg.to_lowercase().contains("document"));
    assert!(!msg.contains(&LOCAL_WORKSPACE_UUID.to_string()));
}

#[test]
fn local_operator_is_never_nil() {
    let ctx = PrincipalContext::local_operator();
    assert!(!ctx.workspace_id().as_uuid().is_nil());
    assert!(!ctx.principal_id().as_uuid().is_nil());
    assert!(ctx.has_scope(Scope::DocumentsRead));
    assert!(ctx.has_scope(Scope::BatchesCreate));
}

#[tokio::test]
async fn repository_find_does_not_return_other_workspace_rows() {
    let pool = connect().await;
    let other = WorkspaceId::new(Uuid::from_u128(0x9999_8888_4777_8666_1555_4444_3333_2222))
        .expect("non-nil");

    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)")
        .bind(other.as_uuid())
        .bind(format!("iso-{}", other.as_uuid().simple()))
        .bind("isolation")
        .execute(&pool)
        .await
        .unwrap();

    let local = WorkspaceId::local();
    let doc = DocumentRepo::create(
        &pool,
        local,
        &format!("iso-{}", Uuid::new_v4().simple()),
        "secret.pdf",
        "pdf",
        "s3/secret",
        12,
        1,
    )
    .await
    .unwrap();

    let leaked = DocumentRepo::find_by_id(&pool, other, doc.id)
        .await
        .unwrap();
    assert!(leaked.is_none(), "cross-workspace find_by_id must be none");

    let listed = DocumentRepo::list_all(&pool, other).await.unwrap();
    assert!(
        listed.iter().all(|d| d.id != doc.id),
        "list_all must not include another workspace's documents"
    );

    let deleted = DocumentRepo::delete_by_id(&pool, other, doc.id)
        .await
        .unwrap();
    assert!(deleted.is_none(), "cross-workspace delete must be a no-op");

    let still_there = DocumentRepo::find_by_id(&pool, local, doc.id)
        .await
        .unwrap();
    assert!(still_there.is_some(), "owner workspace must still see the document");
}

#[tokio::test]
async fn templates_are_workspace_scoped() {
    let pool = connect().await;
    let local = WorkspaceId::local();
    let templates = TemplateRepo::list_all(&pool, local).await.unwrap();
    assert!(
        templates.iter().any(|t| t.is_system),
        "local workspace should own the backfilled system templates"
    );

    let other = WorkspaceId::new(Uuid::from_u128(0x1234_1234_4234_8234_1234_1234_1234_1234))
        .expect("non-nil");
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)")
        .bind(other.as_uuid())
        .bind(format!("tpl-{}", other.as_uuid().simple()))
        .bind("templates")
        .execute(&pool)
        .await
        .unwrap();

    let empty = TemplateRepo::list_all(&pool, other).await.unwrap();
    assert!(
        empty.is_empty(),
        "a new workspace must not inherit another workspace's templates"
    );

    if let Some(system) = templates.into_iter().find(|t| t.is_system) {
        let stolen = TemplateRepo::find_by_id(&pool, other, system.id)
            .await
            .unwrap();
        assert!(stolen.is_none());
    }
}
