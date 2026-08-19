mod worker;

use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client as S3Client;
use chrono::Utc;
use sqlx::PgPool;
use std::time::Duration;
use struxio_common::config::Config;
use struxio_common::mime::normalize_mime_type;
use struxio_core::{
    gemini::GeminiClient,
    gemini::GeminiError,
    queue::redis::{RedisConsumer, RedisConsumerConfig},
    queue::{ExtractionJob, QueueConsumer},
    storage::StorageClient,
};
use struxio_db::repositories::{
    batch_jobs::BatchJobRepo,
    documents::DocumentRepo,
    extractions::{ClaimResult, ExtractionRepo},
    templates::TemplateRepo,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use worker::{
    apply_settlement, settlement_for, FailureClass, ProcessingFailure, RetryPolicy, Settlement,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().json())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env().expect("Failed to load configuration");

    let pool = struxio_db::create_pool(&config.database_url).await?;

    let redis = redis::Client::open(config.redis_url.as_str())?;

    let credentials =
        Credentials::from_keys(&config.s3_access_key_id, &config.s3_secret_access_key, None);

    let s3_config = aws_sdk_s3::Config::builder()
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .region(Region::new(config.s3_region.clone()))
        .endpoint_url(&config.s3_endpoint)
        .credentials_provider(credentials)
        .force_path_style(true)
        .build();

    let s3 = S3Client::from_conf(s3_config);
    let storage = StorageClient::new(s3, config.s3_bucket.clone());

    let gemini = GeminiClient::new(
        config.gemini_api_key.clone(),
        config.gemini_model.clone(),
        Duration::from_secs(config.gemini_timeout_secs),
    )?;

    let consumer_name = format!("worker-{}", uuid::Uuid::new_v4());
    let consumer = std::sync::Arc::new(RedisConsumer::with_config(
        redis,
        "workers".to_string(),
        consumer_name,
        RedisConsumerConfig {
            visibility_timeout: Duration::from_secs(config.queue_visibility_timeout_secs),
            max_attempts: config.queue_max_attempts,
        },
    ));

    consumer.ensure_group().await?;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(config.worker_concurrency));
    let retry_policy = RetryPolicy {
        initial_delay: Duration::from_secs(config.queue_retry_initial_secs),
        max_delay: Duration::from_secs(config.queue_retry_max_secs),
    };
    let visibility_timeout = Duration::from_secs(config.queue_visibility_timeout_secs);
    tracing::info!(
        concurrency = config.worker_concurrency,
        max_attempts = config.queue_max_attempts,
        visibility_timeout_secs = config.queue_visibility_timeout_secs,
        "Worker started, waiting for jobs"
    );

    loop {
        match consumer.next_job().await {
            Ok(Some(job)) => {
                let permit = semaphore.clone().acquire_owned().await?;
                let consumer = consumer.clone();
                let pool = pool.clone();
                let storage = storage.clone();
                let gemini = gemini.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(error) = handle_job(
                        &consumer,
                        &job,
                        &pool,
                        &storage,
                        &gemini,
                        retry_policy,
                        visibility_timeout,
                    )
                    .await
                    {
                        tracing::error!(
                            extraction_id = %job.extraction_id,
                            workspace_id = %job.workspace_id,
                            error = %error,
                            "Job settlement failed; leaving message pending for reclaim"
                        );
                    }
                });
            }
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(error = %e, "Error reading from queue");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

async fn handle_job(
    consumer: &RedisConsumer,
    job: &ExtractionJob,
    pool: &PgPool,
    storage: &StorageClient,
    gemini: &GeminiClient,
    retry_policy: RetryPolicy,
    visibility_timeout: Duration,
) -> anyhow::Result<()> {
    let lease_cutoff = Utc::now()
        - chrono::Duration::from_std(visibility_timeout)
            .map_err(|error| anyhow::anyhow!("visibility timeout: {error}"))?;

    match ExtractionRepo::claim_for_processing(
        pool,
        job.workspace_id,
        job.extraction_id,
        job.attempt,
        lease_cutoff,
    )
    .await?
    {
        ClaimResult::AlreadyProcessing => {
            // Another worker owns the active lease. Keep this duplicate
            // pending; once the owner reaches a durable state, this delivery
            // will be observed as terminal and acknowledged.
            return Ok(());
        }
        ClaimResult::AlreadyTerminal => {
            refresh_batch_progress_if_needed(pool, job).await?;
            consumer.ack(&job.stream_id).await?;
            return Ok(());
        }
        ClaimResult::Claimed => {}
    }

    let failure = match process_claimed_extraction(job, pool, storage, gemini).await {
        Ok(()) => None,
        Err(error) => Some(error),
    };
    let settlement = settlement_for(job, failure.as_ref(), retry_policy);

    match &settlement {
        Settlement::Ack => {
            // process_claimed_extraction has durably written the result and
            // refreshed the workspace-scoped batch before this ACK.
        }
        Settlement::Retry { delay, .. } => {
            let failure = failure
                .as_ref()
                .expect("retry settlement always has a failure");
            let next_retry_at = Utc::now()
                + chrono::Duration::from_std(*delay)
                    .map_err(|error| anyhow::anyhow!("retry delay: {error}"))?;
            ExtractionRepo::mark_retry(
                pool,
                job.workspace_id,
                job.extraction_id,
                "transient",
                &failure.reason,
                next_retry_at,
            )
            .await?;
        }
        Settlement::DeadLetter { .. } => {
            let failure = failure
                .as_ref()
                .expect("DLQ settlement always has a failure");
            let class = match failure.class {
                FailureClass::Retryable => "attempts_exhausted",
                FailureClass::Permanent => "permanent",
            };
            ExtractionRepo::fail_idempotent(
                pool,
                job.workspace_id,
                job.extraction_id,
                &failure.reason,
                class,
            )
            .await?;
            refresh_batch_progress_if_needed(pool, job).await?;
        }
    }

    apply_settlement(consumer, job, settlement).await?;
    Ok(())
}

async fn process_claimed_extraction(
    job: &ExtractionJob,
    pool: &PgPool,
    storage: &StorageClient,
    gemini: &GeminiClient,
) -> Result<(), ProcessingFailure> {
    let workspace_id = job.workspace_id;

    let doc = DocumentRepo::find_by_id(pool, workspace_id, job.document_id)
        .await
        .map_err(|error| ProcessingFailure::retryable(error.to_string()))?
        .ok_or_else(|| ProcessingFailure::permanent("Document not found"))?;

    let template = TemplateRepo::find_by_id(pool, workspace_id, job.template_id)
        .await
        .map_err(|error| ProcessingFailure::retryable(error.to_string()))?
        .ok_or_else(|| ProcessingFailure::permanent("Template not found"))?;

    let file_bytes = storage
        .download(&doc.s3_key)
        .await
        .map_err(|error| ProcessingFailure::retryable(error.to_string()))?;

    let mime_type = normalize_mime_type(&doc.file_type)
        .map_err(|error| ProcessingFailure::permanent(error.to_string()))?;

    let start = std::time::Instant::now();
    let response = gemini
        .extract(
            &file_bytes,
            mime_type,
            &template.prompt_template,
            &template.json_schema,
        )
        .await
        .map_err(classify_gemini_error)?;
    let processing_time = start.elapsed().as_millis() as i32;

    ExtractionRepo::complete_idempotent(
        pool,
        workspace_id,
        job.extraction_id,
        &response.result,
        response.input_tokens,
        response.output_tokens,
        Some(processing_time),
    )
    .await
    .map_err(|error| ProcessingFailure::retryable(error.to_string()))?;

    refresh_batch_progress_if_needed(pool, job)
        .await
        .map_err(|error| ProcessingFailure::retryable(error.to_string()))?;

    Ok(())
}

fn classify_gemini_error(error: GeminiError) -> ProcessingFailure {
    match error {
        GeminiError::Http(error) => ProcessingFailure::retryable(error.to_string()),
        GeminiError::Parse(error) => ProcessingFailure::permanent(error),
        GeminiError::Api(error) => {
            let permanent = [" 400", " 401", " 403", " 404"]
                .iter()
                .any(|status| error.contains(status));
            if permanent {
                ProcessingFailure::permanent(error)
            } else {
                ProcessingFailure::retryable(error)
            }
        }
    }
}

async fn refresh_batch_progress_if_needed(
    pool: &sqlx::PgPool,
    job: &ExtractionJob,
) -> anyhow::Result<()> {
    let Some(batch_id) = job.batch_job_id else {
        return Ok(());
    };
    BatchJobRepo::refresh_progress(pool, job.workspace_id, batch_id).await?;
    Ok(())
}
