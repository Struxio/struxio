// SPDX-License-Identifier: AGPL-3.0-only

//! Schema assertions for the Wave 1C workspace isolation migrations.
//! Requires `DATABASE_URL`. These tests are intended to be run in CI after
//! Postgres is available; they are not executed as part of this change.

use sqlx::PgPool;
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

#[tokio::test]
async fn tenant_tables_have_non_null_workspace_id() {
    let pool = connect().await;
    let tables = [
        "documents",
        "extraction_templates",
        "extractions",
        "batch_jobs",
        "extraction_contract_contents",
        "extraction_contract_versions",
        "extraction_evidence_sidecars",
        "extraction_evidence_entries",
        "extraction_evidence_attachments",
        "extraction_contract_fixtures",
        "extraction_validation_reports",
        "extraction_eval_runs",
        "extraction_eval_run_results",
    ];
    for table in tables {
        let row: (String, String) = sqlx::query_as(
            "SELECT is_nullable, data_type FROM information_schema.columns \
             WHERE table_name = $1 AND column_name = 'workspace_id'",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|_| panic!("workspace_id missing on {table}"));
        assert_eq!(row.0, "NO", "{table}.workspace_id must be NOT NULL");
        assert_eq!(row.1, "uuid");
    }
}

#[tokio::test]
async fn local_workspace_seed_is_not_nil() {
    let pool = connect().await;
    let (id, slug): (uuid::Uuid, String) =
        sqlx::query_as("SELECT id, slug FROM workspaces WHERE slug = 'local'")
            .fetch_one(&pool)
            .await
            .expect("local workspace");
    assert!(!id.is_nil());
    assert_eq!(id, LOCAL_WORKSPACE_UUID);
    assert_eq!(slug, "local");
}

#[tokio::test]
async fn composite_foreign_keys_exist() {
    let pool = connect().await;
    let expected = [
        "extractions_workspace_document_fkey",
        "extractions_workspace_template_fkey",
        "extractions_workspace_batch_job_fkey",
        "batch_jobs_workspace_template_fkey",
    ];
    for name in expected {
        let found: Option<(String,)> = sqlx::query_as(
            "SELECT constraint_name FROM information_schema.table_constraints \
             WHERE constraint_name = $1 AND constraint_type = 'FOREIGN KEY'",
        )
        .bind(name)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(found.is_some(), "missing composite FK {name}");
    }
}

#[tokio::test]
async fn document_hash_unique_is_workspace_local() {
    let pool = connect().await;
    let found: Option<(String,)> = sqlx::query_as(
        "SELECT constraint_name FROM information_schema.table_constraints \
         WHERE constraint_name = 'documents_workspace_id_md5_hash_key'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(found.is_some(), "expected unique (workspace_id, md5_hash)");

    let global: Option<(String,)> = sqlx::query_as(
        "SELECT constraint_name FROM information_schema.table_constraints \
         WHERE constraint_name = 'documents_md5_hash_key'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(
        global.is_none(),
        "global md5 unique constraint must be dropped"
    );
}

#[tokio::test]
async fn published_contract_content_is_hash_addressed() {
    let pool = connect().await;
    let check: Option<(String,)> = sqlx::query_as(
        "SELECT conname FROM pg_constraint \
         WHERE conname = 'extraction_contract_contents_hash_matches_payload'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(
        check.is_some(),
        "content hash must be constrained to payload digest"
    );
}

#[tokio::test]
async fn append_only_triggers_cover_contract_evidence_and_eval_tables() {
    let pool = connect().await;
    let tables = [
        "extraction_contract_contents",
        "extraction_contract_versions",
        "extraction_evidence_sidecars",
        "extraction_evidence_entries",
        "extraction_evidence_attachments",
        "extraction_contract_fixtures",
        "extraction_validation_reports",
        "extraction_eval_runs",
        "extraction_eval_run_results",
    ];
    for table in tables {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.triggers \
             WHERE event_object_table = $1 AND trigger_name LIKE $2",
        )
        .bind(table)
        .bind(format!("{table}_append_only%"))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(count >= 1, "append-only trigger missing on {table}");
    }
}

#[tokio::test]
async fn extractions_have_non_negative_attempt() {
    let pool = connect().await;
    let row: (String, String) = sqlx::query_as(
        "SELECT is_nullable, data_type FROM information_schema.columns \
         WHERE table_name = 'extractions' AND column_name = 'attempt'",
    )
    .fetch_one(&pool)
    .await
    .expect("attempt column");
    assert_eq!(row.0, "NO");
    assert_eq!(row.1, "integer");
}
