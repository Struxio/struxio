use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client as S3Client;
use struxio_core::{
    gemini::GeminiClient,
    queue::{ExtractionJob, QueueConsumer},
    queue::redis::RedisConsumer,
    storage::StorageClient,
};
use struxio_db::repositories::{
    batch_jobs::BatchJobRepo,
    documents::DocumentRepo,
    extractions::ExtractionRepo,
    templates::TemplateRepo,
};
use sqlx::PgPool;
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

    let credentials = Credentials::from_keys(
        &config.s3_access_key_id,
        &config.s3_secret_access_key,
        None,
    );

    let s3_config = aws_sdk_s3::Config::builder()
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .region(Region::new(config.s3_region.clone()))
        .endpoint_url(&config.s3_endpoint)
        .credentials_provider(credentials)
        .force_path_style(true)
        .build();

    let s3 = S3Client::from_conf(s3_config);
    let storage = StorageClient::new(s3, config.s3_bucket.clone());

    let gemini = GeminiClient::new(config.gemini_api_key.clone(), config.gemini_model.clone());

    let consumer_name = format!("worker-{}", uuid::Uuid::new_v4());
    let consumer = RedisConsumer::new(redis, "workers".to_string(), consumer_name);

    consumer.ensure_group().await?;
    tracing::info!("Worker started, waiting for jobs...");

    loop {
        match consumer.next_job().await {
            Ok(Some(job)) => {
                tracing::info!(extraction_id = %job.extraction_id, "Processing extraction");
                match process_extraction(&job, &pool, &storage, &gemini).await {
                    Ok(()) => {
                        consumer.ack(&job.stream_id).await?;
                        tracing::info!(extraction_id = %job.extraction_id, "Extraction completed");
                    }
                    Err(e) => {
                        tracing::error!(extraction_id = %job.extraction_id, error = %e, "Extraction failed");
                        ExtractionRepo::update_failed(&pool, job.extraction_id, &e.to_string())
                            .await
                            .ok();
                        update_batch_progress_if_needed(&pool, job.batch_job_id).await;
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
    gemini: &GeminiClient,
) -> anyhow::Result<()> {
    ExtractionRepo::update_status(pool, job.extraction_id, "processing").await?;

    let doc = DocumentRepo::find_by_id(pool, job.document_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let template = TemplateRepo::find_by_id(pool, job.template_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Template not found"))?;

    let file_bytes = storage.download(&doc.s3_key).await?;

    let mime_type = match doc.file_type.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    };

    let start = std::time::Instant::now();
    let response = gemini
        .extract(
            &file_bytes,
            mime_type,
            &template.prompt_template,
            &template.json_schema,
        )
        .await?;
    let processing_time = start.elapsed().as_millis() as i32;

    ExtractionRepo::update_result(
        pool,
        job.extraction_id,
        &response.result,
        response.input_tokens,
        response.output_tokens,
        Some(processing_time),
    )
    .await?;

    update_batch_progress_if_needed(pool, job.batch_job_id).await;

    Ok(())
}

async fn update_batch_progress_if_needed(
    pool: &sqlx::PgPool,
    batch_job_id: Option<uuid::Uuid>,
) {
    let Some(batch_id) = batch_job_id else {
        return;
    };
    let Ok((completed, failed)) =
        ExtractionRepo::count_by_batch_status(pool, batch_id).await
    else {
        return;
    };
    let _ = BatchJobRepo::update_progress(pool, batch_id, completed, failed, "processing").await;
}
