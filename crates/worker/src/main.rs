use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client as S3Client;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use struxio_common::mime::normalize_mime_type;
use struxio_common::JobExecutionContext;
use struxio_core::{
    gemini::GeminiClient,
    provider::{ExtractionProvider, ExtractionRequest, SharedExtractionProvider},
    queue::redis::RedisConsumer,
    queue::{ExtractionJob, QueueConsumer},
    storage::StorageClient,
};
use struxio_db::repositories::{
    batch_jobs::BatchJobRepo, documents::DocumentRepo, extractions::ExtractionRepo,
    templates::TemplateRepo,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().json())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = struxio_common::config::Config::from_env().expect("Failed to load configuration");

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
    let consumer = RedisConsumer::new(redis, "workers".to_string(), consumer_name);

    consumer.ensure_group().await?;
    tracing::info!("Worker started, waiting for jobs...");

    loop {
        match consumer.next_job().await {
            Ok(Some(job)) => {
                tracing::info!(extraction_id = %job.extraction_id, workspace_id = %job.workspace_id, "Processing extraction");
                match process_extraction(&job, &pool, &storage, provider.as_ref()).await {
                    Ok(()) => {
                        consumer.ack(&job.stream_id).await?;
                        tracing::info!(extraction_id = %job.extraction_id, "Extraction completed");
                    }
                    Err(e) => {
                        tracing::error!(extraction_id = %job.extraction_id, error = %e, "Extraction failed");
                        ExtractionRepo::update_failed(
                            &pool,
                            job.workspace_id,
                            job.extraction_id,
                            &e.to_string(),
                        )
                        .await
                        .ok();
                        update_batch_progress_if_needed(&pool, &job).await;
                        consumer.ack(&job.stream_id).await?;
                    }
                }
            }
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(error = %e, "Error reading from queue");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

async fn process_extraction(
    job: &ExtractionJob,
    pool: &PgPool,
    storage: &StorageClient,
    provider: &dyn ExtractionProvider,
) -> anyhow::Result<()> {
    let ctx = JobExecutionContext::new(job.workspace_id);
    let workspace_id = ctx.workspace_id();

    ExtractionRepo::update_status(pool, workspace_id, job.extraction_id, "processing").await?;

    let doc = DocumentRepo::find_by_id(pool, workspace_id, job.document_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let template = TemplateRepo::find_by_id(pool, workspace_id, job.template_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Template not found"))?;

    let file_bytes = storage.download(&doc.s3_key).await?;

    let mime_type = normalize_mime_type(&doc.file_type)?;

    let start = std::time::Instant::now();
    let response = provider
        .extract(ExtractionRequest {
            bytes: &file_bytes,
            mime_type,
            instructions: &template.prompt_template,
            json_schema: &template.json_schema,
        })
        .await?;
    let processing_time = start.elapsed().as_millis() as i32;

    ExtractionRepo::update_result(
        pool,
        workspace_id,
        job.extraction_id,
        &response.result,
        response.usage.input_tokens,
        response.usage.output_tokens,
        Some(processing_time),
    )
    .await?;

    update_batch_progress_if_needed(pool, job).await;

    Ok(())
}

async fn update_batch_progress_if_needed(pool: &sqlx::PgPool, job: &ExtractionJob) {
    let Some(batch_id) = job.batch_job_id else {
        return;
    };
    let Ok((completed, failed)) =
        ExtractionRepo::count_by_batch_status(pool, job.workspace_id, batch_id).await
    else {
        return;
    };
    let _ = BatchJobRepo::update_progress(
        pool,
        job.workspace_id,
        batch_id,
        completed,
        failed,
        "processing",
    )
    .await;
}
