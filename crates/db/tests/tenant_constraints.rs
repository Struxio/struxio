// SPDX-License-Identifier: AGPL-3.0-only

//! Composite FK and workspace-local unique constraint coverage.
//! Requires `DATABASE_URL`. Not executed as part of this change.

use sqlx::PgPool;
use uuid::Uuid;

use struxio_common::LOCAL_WORKSPACE_UUID;

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

async fn insert_workspace(pool: &PgPool, id: Uuid, slug: &str) {
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(slug)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert workspace");
}

#[tokio::test]
async fn same_md5_allowed_in_two_workspaces() {
    let pool = connect().await;
    let other = Uuid::from_u128(0x1111_2222_4333_8444_1555_6666_7777_8888);
    insert_workspace(&pool, other, &format!("other-{}", other.simple())).await;

    let hash = format!("md5-{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'a.pdf', 'pdf', 'a', 1)",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(&hash)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'b.pdf', 'pdf', 'b', 1)",
    )
    .bind(other)
    .bind(&hash)
    .execute(&pool)
    .await
    .expect("same hash in another workspace must succeed");
}

#[tokio::test]
async fn duplicate_md5_rejected_inside_one_workspace() {
    let pool = connect().await;
    let hash = format!("md5-dup-{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'a.pdf', 'pdf', 'a', 1)",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(&hash)
    .execute(&pool)
    .await
    .unwrap();

    let err = sqlx::query(
        "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'b.pdf', 'pdf', 'b', 1)",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(&hash)
    .execute(&pool)
    .await
    .expect_err("duplicate hash in the same workspace must fail");
    assert!(err.to_string().contains("documents_workspace_id_md5_hash_key") || err.to_string().contains("duplicate"));
}

#[tokio::test]
async fn composite_fk_rejects_cross_workspace_document_reference() {
    let pool = connect().await;
    let other = Uuid::from_u128(0xaaaa_bbbb_4ccc_8ddd_eeee_ffff_0000_1111);
    insert_workspace(&pool, other, &format!("fk-other-{}", other.simple())).await;

    let doc_id: Uuid = sqlx::query_scalar(
        "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'a.pdf', 'pdf', 'a', 1) RETURNING id",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(format!("md5-{}", Uuid::new_v4().simple()))
    .fetch_one(&pool)
    .await
    .unwrap();

    let template_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM extraction_templates WHERE workspace_id = $1 LIMIT 1",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .fetch_one(&pool)
    .await
    .expect("local system template");

    // Copy a template into the other workspace so template_id FK can be satisfied
    // while the document stays in the local workspace.
    let other_template: Uuid = sqlx::query_scalar(
        "INSERT INTO extraction_templates (workspace_id, name, json_schema, prompt_template, is_system) \
         VALUES ($1, 'Invoice', '{}'::jsonb, 'x', true) RETURNING id",
    )
    .bind(other)
    .fetch_one(&pool)
    .await
    .unwrap();

    let err = sqlx::query(
        "INSERT INTO extractions (workspace_id, document_id, template_id, status) \
         VALUES ($1, $2, $3, 'pending')",
    )
    .bind(other)
    .bind(doc_id)
    .bind(other_template)
    .execute(&pool)
    .await
    .expect_err("cross-workspace document FK must fail");
    assert!(
        err.to_string().contains("extractions_workspace_document_fkey")
            || err.to_string().to_lowercase().contains("foreign key")
    );

    let _ = template_id;
}

#[tokio::test]
async fn nil_workspace_id_is_rejected() {
    let pool = connect().await;
    let err = sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, 'nil', 'nil')")
        .bind(Uuid::nil())
        .execute(&pool)
        .await
        .expect_err("nil workspace id must fail");
    assert!(
        err.to_string().contains("workspaces_id_not_nil")
            || err.to_string().to_lowercase().contains("check")
    );
}

#[tokio::test]
async fn deleting_batch_clears_only_batch_reference() {
    let pool = connect().await;
    let template_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM extraction_templates WHERE workspace_id = $1 LIMIT 1",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .fetch_one(&pool)
    .await
    .expect("local system template");

    let doc_id: Uuid = sqlx::query_scalar(
        "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'batch.pdf', 'application/pdf', $3, 1) RETURNING id",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(format!("batch-delete-{}", Uuid::new_v4().simple()))
    .bind(format!("batch-delete/{}", Uuid::new_v4()))
    .fetch_one(&pool)
    .await
    .expect("document");

    let batch_id: Uuid = sqlx::query_scalar(
        "INSERT INTO batch_jobs (workspace_id, template_id, total_documents) \
         VALUES ($1, $2, 1) RETURNING id",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(template_id)
    .fetch_one(&pool)
    .await
    .expect("batch");

    let extraction_id: Uuid = sqlx::query_scalar(
        "INSERT INTO extractions \
         (workspace_id, document_id, template_id, batch_job_id, status) \
         VALUES ($1, $2, $3, $4, 'pending') RETURNING id",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(doc_id)
    .bind(template_id)
    .bind(batch_id)
    .fetch_one(&pool)
    .await
    .expect("extraction");

    sqlx::query("DELETE FROM batch_jobs WHERE workspace_id = $1 AND id = $2")
        .bind(LOCAL_WORKSPACE_UUID)
        .bind(batch_id)
        .execute(&pool)
        .await
        .expect("batch deletion should preserve the extraction");

    let (workspace_id, batch_job_id): (Uuid, Option<Uuid>) = sqlx::query_as(
        "SELECT workspace_id, batch_job_id FROM extractions WHERE id = $1",
    )
    .bind(extraction_id)
    .fetch_one(&pool)
    .await
    .expect("preserved extraction");
    assert_eq!(workspace_id, LOCAL_WORKSPACE_UUID);
    assert!(batch_job_id.is_none());
}
