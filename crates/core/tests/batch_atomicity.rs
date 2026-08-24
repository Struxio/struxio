use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use struxio_common::models::CreateBatchRequest;
use struxio_common::{AppError, PrincipalContext, WorkspaceId, LOCAL_WORKSPACE_UUID};
use struxio_core::queue::{QueueError, QueueProducer};
use struxio_core::services::batch_service::BatchService;
use struxio_core::services::outbox_service::flush_batch_outbox;
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

async fn template_id(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM extraction_templates WHERE workspace_id = $1 LIMIT 1")
        .bind(LOCAL_WORKSPACE_UUID)
        .fetch_one(pool)
        .await
        .expect("local template")
}

async fn document_id(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO documents \
         (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'batch.pdf', 'application/pdf', $3, 1) RETURNING id",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(format!("batch-atomic-{}", Uuid::new_v4()))
    .bind(format!("{}/batch/{}", LOCAL_WORKSPACE_UUID, Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .expect("document")
}

#[derive(Clone)]
struct RecordingQueue {
    successful_before_failure: Option<usize>,
    calls: Arc<AtomicUsize>,
}

impl RecordingQueue {
    fn succeeding() -> Self {
        Self {
            successful_before_failure: None,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn fail_after(successes: usize) -> Self {
        Self {
            successful_before_failure: Some(successes),
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl QueueProducer for RecordingQueue {
    async fn enqueue_extraction(
        &self,
        _extraction_id: Uuid,
        _document_id: Uuid,
        _template_id: Uuid,
        _workspace_id: WorkspaceId,
        _batch_job_id: Option<Uuid>,
    ) -> Result<(), QueueError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self
            .successful_before_failure
            .is_some_and(|successes| call >= successes)
        {
            Err(QueueError::Other("queue unavailable".to_string()))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn missing_document_rolls_back_entire_batch() {
    let pool = connect().await;
    let template_id = template_id(&pool).await;
    let valid_document = document_id(&pool).await;
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM batch_jobs WHERE workspace_id = $1")
        .bind(LOCAL_WORKSPACE_UUID)
        .fetch_one(&pool)
        .await
        .unwrap();
    let queue = RecordingQueue::succeeding();
    let service = BatchService::new(pool.clone(), queue.clone());

    let result = service
        .create(
            &PrincipalContext::local_operator(),
            &CreateBatchRequest {
                template_id,
                document_ids: vec![valid_document, Uuid::new_v4()],
            },
            "gemini-2.5-flash",
        )
        .await;
    assert!(matches!(result, Err(AppError::NotFound(_))));

    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM batch_jobs WHERE workspace_id = $1")
        .bind(LOCAL_WORKSPACE_UUID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(queue.calls(), 0);
}

#[tokio::test]
async fn enqueue_failure_leaves_complete_batch_in_retryable_outbox() {
    let pool = connect().await;
    let template_id = template_id(&pool).await;
    let documents = vec![
        document_id(&pool).await,
        document_id(&pool).await,
        document_id(&pool).await,
    ];
    let queue = RecordingQueue::fail_after(1);
    let service = BatchService::new(pool.clone(), queue);
    let batch = service
        .create(
            &PrincipalContext::local_operator(),
            &CreateBatchRequest {
                template_id,
                document_ids: documents,
            },
            "gemini-2.5-flash",
        )
        .await
        .expect("durable batch");

    let extraction_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM extractions WHERE batch_job_id = $1")
            .bind(batch.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let (published, pending): (i64, i64) = sqlx::query_as(
        "SELECT \
            COUNT(*) FILTER (WHERE published_at IS NOT NULL), \
            COUNT(*) FILTER (WHERE published_at IS NULL) \
         FROM extraction_outbox WHERE batch_job_id = $1",
    )
    .bind(batch.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(extraction_count, 3);
    assert_eq!((published, pending), (1, 2));

    let retry_queue = RecordingQueue::succeeding();
    let flush = flush_batch_outbox(&pool, &retry_queue, WorkspaceId::local(), batch.id)
        .await
        .expect("retry outbox");
    assert_eq!(flush.published, 2);
    assert_eq!(flush.failed, 0);
    assert_eq!(retry_queue.calls(), 2);

    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM extraction_outbox \
         WHERE batch_job_id = $1 AND published_at IS NULL",
    )
    .bind(batch.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pending, 0);
}
