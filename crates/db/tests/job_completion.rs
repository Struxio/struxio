// SPDX-License-Identifier: AGPL-3.0-only

//! Transactional batch-counter and idempotent terminal-apply coverage.
//! Requires `DATABASE_URL`.

use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use struxio_common::{WorkspaceId, LOCAL_WORKSPACE_UUID};
use struxio_db::repositories::extractions::{ClaimOutcome, ExtractionRepo};
use uuid::Uuid;

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

async fn fixture(pool: &PgPool, children: usize) -> (WorkspaceId, Uuid, Vec<Uuid>) {
    let workspace = WorkspaceId::local();
    let template_id: Uuid =
        sqlx::query_scalar("SELECT id FROM extraction_templates WHERE workspace_id = $1 LIMIT 1")
            .bind(LOCAL_WORKSPACE_UUID)
            .fetch_one(pool)
            .await
            .expect("local system template");

    let batch_id: Uuid = sqlx::query_scalar(
        "INSERT INTO batch_jobs (workspace_id, template_id, total_documents) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(template_id)
    .bind(children as i32)
    .fetch_one(pool)
    .await
    .expect("batch");

    let mut extraction_ids = Vec::new();
    for _ in 0..children {
        let doc_id: Uuid = sqlx::query_scalar(
            "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
             VALUES ($1, $2, 'job.pdf', 'application/pdf', $3, 1) RETURNING id",
        )
        .bind(LOCAL_WORKSPACE_UUID)
        .bind(format!("job-{}", Uuid::new_v4().simple()))
        .bind(format!("job/{}", Uuid::new_v4()))
        .fetch_one(pool)
        .await
        .expect("document");

        let extraction_id: Uuid = sqlx::query_scalar(
            "INSERT INTO extractions \
             (workspace_id, document_id, template_id, batch_job_id, status) \
             VALUES ($1, $2, $3, $4, 'processing') RETURNING id",
        )
        .bind(LOCAL_WORKSPACE_UUID)
        .bind(doc_id)
        .bind(template_id)
        .bind(batch_id)
        .fetch_one(pool)
        .await
        .expect("extraction");
        extraction_ids.push(extraction_id);
    }

    (workspace, batch_id, extraction_ids)
}

async fn batch_row(pool: &PgPool, batch_id: Uuid) -> (String, i32, i32) {
    sqlx::query_as(
        "SELECT status, completed_documents, failed_documents FROM batch_jobs WHERE id = $1",
    )
    .bind(batch_id)
    .fetch_one(pool)
    .await
    .expect("batch row")
}

#[tokio::test]
async fn migrated_legacy_processing_row_is_reclaimable() {
    let pool = connect().await;
    let (workspace, _batch_id, ids) = fixture(&pool, 1).await;
    let before: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT processing_lease_expires_at FROM extractions WHERE id = $1")
            .bind(ids[0])
            .fetch_one(&pool)
            .await
            .expect("legacy lease");
    assert!(before.is_none());

    sqlx::query(
        "UPDATE extractions SET processing_lease_expires_at = now() \
         WHERE id = $1 AND status = 'processing' \
           AND processing_lease_expires_at IS NULL",
    )
    .bind(ids[0])
    .execute(&pool)
    .await
    .expect("backfill legacy lease");

    let claimed = ExtractionRepo::claim_for_processing(
        &pool,
        workspace,
        ids[0],
        Uuid::new_v4(),
        Duration::from_secs(60),
    )
    .await
    .expect("claim legacy row");
    let ClaimOutcome::Run(extraction) = claimed else {
        panic!("expired legacy row must run");
    };
    assert_eq!(extraction.attempt, 0);
}

#[tokio::test]
async fn processing_lease_allows_only_one_live_claim() {
    let pool = connect().await;
    let (workspace, _batch_id, ids) = fixture(&pool, 1).await;
    sqlx::query(
        "UPDATE extractions SET status = 'pending', processing_lease_expires_at = NULL, \
                processing_lease_token = NULL \
         WHERE id = $1",
    )
    .bind(ids[0])
    .execute(&pool)
    .await
    .expect("reset pending");

    let first_token = Uuid::new_v4();
    let second_token = Uuid::new_v4();
    let first = ExtractionRepo::claim_for_processing(
        &pool,
        workspace,
        ids[0],
        first_token,
        Duration::from_secs(60),
    );
    let second = ExtractionRepo::claim_for_processing(
        &pool,
        workspace,
        ids[0],
        second_token,
        Duration::from_secs(60),
    );
    let (first, second) = tokio::join!(first, second);
    let outcomes = [first.expect("first claim"), second.expect("second claim")];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ClaimOutcome::Run(_)))
            .count(),
        1
    );
    let live_token = if matches!(outcomes[0], ClaimOutcome::Run(_)) {
        first_token
    } else {
        second_token
    };
    assert!(!ExtractionRepo::renew_processing_lease(
        &pool,
        workspace,
        ids[0],
        Uuid::new_v4(),
        Duration::from_secs(60),
    )
    .await
    .unwrap());
    assert!(ExtractionRepo::renew_processing_lease(
        &pool,
        workspace,
        ids[0],
        live_token,
        Duration::from_secs(60),
    )
    .await
    .unwrap());
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ClaimOutcome::AlreadyProcessing(_)))
            .count(),
        1
    );

    let stored = ExtractionRepo::find_by_id(&pool, workspace, ids[0])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.attempt, 1);

    sqlx::query(
        "UPDATE extractions SET processing_lease_expires_at = now() - interval '1 second' \
         WHERE id = $1",
    )
    .bind(ids[0])
    .execute(&pool)
    .await
    .expect("expire lease");
    let reclaimed_token = Uuid::new_v4();
    let reclaimed = ExtractionRepo::claim_for_processing(
        &pool,
        workspace,
        ids[0],
        reclaimed_token,
        Duration::from_secs(60),
    )
    .await
    .expect("reclaim");
    assert!(matches!(reclaimed, ClaimOutcome::Run(_)));
    let stored = ExtractionRepo::find_by_id(&pool, workspace, ids[0])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.attempt, 1, "lease reclaim must not burn an attempt");

    let stale = ExtractionRepo::apply_completed_claimed(
        &pool,
        workspace,
        ids[0],
        live_token,
        &json!({"stale": true}),
        1,
        1,
        1,
        "gemini-2.5-flash",
    )
    .await
    .expect("stale completion is fenced");
    assert!(!stale.changed);
    assert_eq!(stale.extraction.status, "processing");
}

#[tokio::test]
async fn concurrent_completions_are_counted_once_each() {
    let pool = connect().await;
    let (workspace, batch_id, ids) = fixture(&pool, 2).await;
    let result = json!({"ok": true});

    let a = ExtractionRepo::apply_completed(
        &pool,
        workspace,
        ids[0],
        &result,
        1,
        1,
        10,
        "gemini-2.5-flash",
    );
    let b = ExtractionRepo::apply_completed(
        &pool,
        workspace,
        ids[1],
        &result,
        1,
        1,
        10,
        "gemini-2.5-flash",
    );
    let (first, second) = tokio::join!(a, b);
    let first = first.expect("first complete");
    let second = second.expect("second complete");
    assert!(first.changed);
    assert!(second.changed);

    let (status, completed, failed) = batch_row(&pool, batch_id).await;
    assert_eq!(completed, 2);
    assert_eq!(failed, 0);
    assert_eq!(status, "completed");
}

#[tokio::test]
async fn second_terminal_apply_is_idempotent() {
    let pool = connect().await;
    let (workspace, batch_id, ids) = fixture(&pool, 1).await;
    let result = json!({"ok": true});

    let first = ExtractionRepo::apply_completed(
        &pool,
        workspace,
        ids[0],
        &result,
        1,
        1,
        10,
        "gemini-2.5-flash",
    )
    .await
    .expect("complete");
    assert!(first.changed);

    let second = ExtractionRepo::apply_completed(
        &pool,
        workspace,
        ids[0],
        &result,
        9,
        9,
        99,
        "gemini-2.5-flash",
    )
    .await
    .expect("repeat complete");
    assert!(!second.changed);
    assert_eq!(second.extraction.input_tokens, 1);

    let failed = ExtractionRepo::apply_failed(&pool, workspace, ids[0], "should not overwrite")
        .await
        .expect("repeat fail");
    assert!(!failed.changed);
    assert_eq!(failed.extraction.status, "completed");

    let (status, completed, failed_count) = batch_row(&pool, batch_id).await;
    assert_eq!(status, "completed");
    assert_eq!(completed, 1);
    assert_eq!(failed_count, 0);
}

#[tokio::test]
async fn retrying_child_keeps_batch_processing() {
    let pool = connect().await;
    let (workspace, batch_id, ids) = fixture(&pool, 2).await;
    let result = json!({"ok": true});

    ExtractionRepo::apply_completed(
        &pool,
        workspace,
        ids[0],
        &result,
        1,
        1,
        10,
        "gemini-2.5-flash",
    )
    .await
    .expect("complete one");

    ExtractionRepo::mark_retrying(&pool, workspace, ids[1], "provider 429")
        .await
        .expect("retrying");

    struxio_db::repositories::batch_jobs::BatchJobRepo::recompute_progress(
        &pool, workspace, batch_id,
    )
    .await
    .expect("recompute");

    let (status, completed, failed) = batch_row(&pool, batch_id).await;
    assert_eq!(status, "processing");
    assert_eq!(completed, 1);
    assert_eq!(failed, 0);
}

#[tokio::test]
async fn mixed_terminal_children_are_partially_completed() {
    let pool = connect().await;
    let (workspace, batch_id, ids) = fixture(&pool, 2).await;
    let result = json!({"ok": true});

    ExtractionRepo::apply_completed(
        &pool,
        workspace,
        ids[0],
        &result,
        1,
        1,
        10,
        "gemini-2.5-flash",
    )
    .await
    .expect("complete");
    ExtractionRepo::apply_failed(&pool, workspace, ids[1], "permanent")
        .await
        .expect("fail");

    let (status, completed, failed) = batch_row(&pool, batch_id).await;
    assert_eq!(status, "partially_completed");
    assert_eq!(completed, 1);
    assert_eq!(failed, 1);
}
