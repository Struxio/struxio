use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client as S3Client;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use struxio_common::config::Config;
use struxio_common::mime::normalize_mime_type;
use struxio_common::JobExecutionContext;
use struxio_core::{
    gemini::GeminiClient,
    jobs::{event_for_error, step, DeliveryAction, JobError, JobSnapshot, JobStatus, RetryPolicy},
    provider::{ExtractionRequest, ProviderError, SharedExtractionProvider},
    queue::redis::RedisConsumer,
    queue::{ExtractionJob, QueueConsumer, StreamDelivery},
    storage::StorageClient,
};
use struxio_db::repositories::{
    documents::DocumentRepo,
    extractions::{ClaimOutcome, ExtractionRepo},
    templates::TemplateRepo,
};
use tokio::sync::Semaphore;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
    let provider: SharedExtractionProvider = Arc::new(gemini);

    let consumer_name = format!("worker-{}", uuid::Uuid::new_v4());
    let consumer = RedisConsumer::for_workers(redis, consumer_name);
    consumer.ensure_group().await?;

    let policy = RetryPolicy::new(
        config.worker_max_attempts,
        Duration::from_millis(config.worker_initial_backoff_ms),
        Duration::from_millis(config.worker_max_backoff_ms),
        struxio_core::jobs::retry::DEFAULT_JITTER_RATIO,
    );
    let runtime = Arc::new(WorkerRuntime {
        pool,
        storage,
        provider,
        consumer,
        policy,
        model_id: config.gemini_model.clone(),
        concurrency: config.worker_concurrency,
        claim_idle: config.worker_claim_idle(),
    });

    tracing::info!(
        concurrency = runtime.concurrency,
        max_attempts = runtime.policy.max_attempts,
        "Worker started, waiting for jobs..."
    );

    let semaphore = Arc::new(Semaphore::new(runtime.concurrency));
    loop {
        if let Err(e) = runtime.consumer.promote_delayed(runtime.concurrency).await {
            tracing::error!(error = %e, "Failed to promote delayed retries");
        }

        match fill_inbox(&runtime).await {
            Ok(inbox) if inbox.is_empty() => continue,
            Ok(inbox) => {
                for delivery in inbox {
                    let permit = match semaphore.clone().acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => break,
                    };
                    let runtime = Arc::clone(&runtime);
                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Err(e) = handle_delivery(&runtime, delivery).await {
                            tracing::error!(error = %e, "Job handler failed before ACK");
                        }
                    });
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Error reading from queue");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

struct WorkerRuntime {
    pool: PgPool,
    storage: StorageClient,
    provider: SharedExtractionProvider,
    consumer: RedisConsumer,
    policy: RetryPolicy,
    model_id: String,
    concurrency: usize,
    claim_idle: Duration,
}

async fn fill_inbox(
    runtime: &WorkerRuntime,
) -> Result<Vec<StreamDelivery>, struxio_core::queue::QueueError> {
    let mut inbox = runtime
        .consumer
        .reclaim_stale(runtime.claim_idle, runtime.concurrency)
        .await?;
    if inbox.len() < runtime.concurrency {
        let more = runtime
            .consumer
            .read_new(runtime.concurrency - inbox.len(), Duration::from_secs(5))
            .await?;
        inbox.extend(more);
    }
    Ok(inbox)
}

async fn handle_delivery(runtime: &WorkerRuntime, delivery: StreamDelivery) -> anyhow::Result<()> {
    match delivery {
        StreamDelivery::Malformed(message) => {
            tracing::error!(stream_id = %message.stream_id, reason = %message.reason, "Malformed job");
            runtime.consumer.dead_letter_malformed(&message).await?;
            runtime.consumer.ack(&message.stream_id).await?;
            Ok(())
        }
        StreamDelivery::Job(job) => handle_job(runtime, job).await,
    }
}

async fn handle_job(runtime: &WorkerRuntime, job: ExtractionJob) -> anyhow::Result<()> {
    tracing::info!(
        extraction_id = %job.extraction_id,
        workspace_id = %job.workspace_id,
        "Processing extraction"
    );

    match ExtractionRepo::claim_for_processing(&runtime.pool, job.workspace_id, job.extraction_id)
        .await
    {
        Ok(ClaimOutcome::Missing) => {
            runtime
                .consumer
                .dead_letter_job(&job, "extraction row not found")
                .await?;
            runtime.consumer.ack(&job.stream_id).await?;
            Ok(())
        }
        Ok(ClaimOutcome::SkipTerminal(existing)) => {
            tracing::info!(
                extraction_id = %job.extraction_id,
                status = %existing.status,
                "Skipping already-terminal extraction"
            );
            runtime.consumer.ack(&job.stream_id).await?;
            Ok(())
        }
        Ok(ClaimOutcome::Run(claimed)) => {
            let snapshot = snapshot_of(&claimed);
            if snapshot.attempt > runtime.policy.max_attempts {
                finish_dead_letter(runtime, &job, snapshot, "attempts exhausted").await?;
                Ok(())
            } else {
                match process_extraction(&job, runtime).await {
                    Ok(success) => {
                        ExtractionRepo::apply_completed(
                            &runtime.pool,
                            job.workspace_id,
                            job.extraction_id,
                            &success.result,
                            success.input_tokens,
                            success.output_tokens,
                            success.processing_time_ms,
                            &runtime.model_id,
                        )
                        .await?;
                        runtime.consumer.ack(&job.stream_id).await?;
                        tracing::info!(extraction_id = %job.extraction_id, "Extraction completed");
                        Ok(())
                    }
                    Err(error) => {
                        let decision = step(
                            snapshot,
                            event_for_error(&error),
                            runtime.policy,
                            rand_jitter(),
                        );
                        apply_failure_decision(runtime, &job, decision.action, error.message())
                            .await
                    }
                }
            }
        }
        Err(e) => Err(e.into()),
    }
}

struct ExtractionSuccess {
    result: serde_json::Value,
    input_tokens: i32,
    output_tokens: i32,
    processing_time_ms: i32,
}

async fn process_extraction(
    job: &ExtractionJob,
    runtime: &WorkerRuntime,
) -> Result<ExtractionSuccess, JobError> {
    let ctx = JobExecutionContext::new(job.workspace_id);
    let workspace_id = ctx.workspace_id();

    let doc = DocumentRepo::find_by_id(&runtime.pool, workspace_id, job.document_id)
        .await
        .map_err(|e| JobError::retryable(e.to_string()))?
        .ok_or_else(|| JobError::permanent("Document not found"))?;

    let template = TemplateRepo::find_by_id(&runtime.pool, workspace_id, job.template_id)
        .await
        .map_err(|e| JobError::retryable(e.to_string()))?
        .ok_or_else(|| JobError::permanent("Template not found"))?;

    if doc.workspace_id != workspace_id || template.workspace_id != workspace_id {
        return Err(JobError::permanent(
            "job payload workspace does not match stored records",
        ));
    }

    let file_bytes = runtime
        .storage
        .download(&doc.s3_key)
        .await
        .map_err(|e| JobError::retryable(e.to_string()))?;

    let mime_type =
        normalize_mime_type(&doc.file_type).map_err(|e| JobError::permanent(e.to_string()))?;

    let start = std::time::Instant::now();
    let response = runtime
        .provider
        .extract(ExtractionRequest {
            bytes: &file_bytes,
            mime_type,
            instructions: &template.prompt_template,
            json_schema: &template.json_schema,
        })
        .await
        .map_err(provider_job_error)?;

    Ok(ExtractionSuccess {
        result: response.result,
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        processing_time_ms: start.elapsed().as_millis() as i32,
    })
}

fn provider_job_error(error: ProviderError) -> JobError {
    match error {
        ProviderError::Timeout
        | ProviderError::Transient(_)
        | ProviderError::StructuredOutput(_) => JobError::retryable(error.to_string()),
        ProviderError::UnsupportedMediaType(_)
        | ProviderError::Incompatible { .. }
        | ProviderError::Backend(_) => JobError::permanent(error.to_string()),
    }
}

async fn apply_failure_decision(
    runtime: &WorkerRuntime,
    job: &ExtractionJob,
    action: DeliveryAction,
    error_message: &str,
) -> anyhow::Result<()> {
    match action {
        DeliveryAction::AckRetry { delay } => {
            tracing::warn!(
                extraction_id = %job.extraction_id,
                delay_ms = delay.as_millis() as u64,
                error = error_message,
                "Retrying extraction"
            );
            ExtractionRepo::mark_retrying(
                &runtime.pool,
                job.workspace_id,
                job.extraction_id,
                error_message,
            )
            .await?;
            runtime
                .consumer
                .schedule_retry(&job.envelope(), delay)
                .await?;
            runtime.consumer.ack(&job.stream_id).await?;
            Ok(())
        }
        DeliveryAction::AckDeadLetter | DeliveryAction::AckMalformed => {
            finish_dead_letter(
                runtime,
                job,
                JobSnapshot {
                    status: JobStatus::Failed,
                    attempt: 0,
                },
                error_message,
            )
            .await
        }
        DeliveryAction::AckSkip => {
            runtime.consumer.ack(&job.stream_id).await?;
            Ok(())
        }
        DeliveryAction::AckComplete | DeliveryAction::Execute => {
            anyhow::bail!("unexpected delivery action for a failed job: {action:?}")
        }
    }
}

async fn finish_dead_letter(
    runtime: &WorkerRuntime,
    job: &ExtractionJob,
    _snapshot: JobSnapshot,
    error_message: &str,
) -> anyhow::Result<()> {
    tracing::error!(
        extraction_id = %job.extraction_id,
        error = error_message,
        "Extraction dead-lettered"
    );
    ExtractionRepo::apply_failed(
        &runtime.pool,
        job.workspace_id,
        job.extraction_id,
        error_message,
    )
    .await?;
    runtime.consumer.dead_letter_job(job, error_message).await?;
    runtime.consumer.ack(&job.stream_id).await?;
    Ok(())
}

fn snapshot_of(extraction: &struxio_common::models::Extraction) -> JobSnapshot {
    JobSnapshot {
        status: JobStatus::parse(&extraction.status).unwrap_or(JobStatus::Processing),
        attempt: u32::try_from(extraction.attempt.max(0)).unwrap_or(u32::MAX),
    }
}

fn rand_jitter() -> f64 {
    use rand::Rng;
    rand::thread_rng().gen::<f64>()
}
