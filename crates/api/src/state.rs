use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client as S3Client;
use sqlx::PgPool;
use std::time::Duration;
use struxio_common::config::Config;
use struxio_common::PrincipalContext;
use struxio_db::repositories::workspaces::WorkspaceRepo;
use struxio_core::{
    gemini::GeminiClient,
    queue::redis::RedisProducer,
    services::{
        batch_service::BatchService,
        document_service::DocumentService,
        extraction_service::ExtractionService,
        model_service::ModelService,
        template_service::TemplateService,
    },
    storage::StorageClient,
};


#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub redis: redis::Client,
    pub document_service: DocumentService,
    pub template_service: TemplateService,
    pub extraction_service: ExtractionService<RedisProducer>,
    pub batch_service: BatchService<RedisProducer>,
    pub model_service: ModelService,
    pub gemini: GeminiClient,
    /// Seeded local OSS operator. Loaded from Postgres, never a nil UUID.
    pub local_principal: PrincipalContext,
}

impl AppState {
    pub async fn new(config: Config, db_pool: PgPool) -> anyhow::Result<Self> {
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
        let queue = RedisProducer::new(redis.clone());
        let gemini = GeminiClient::new(
            config.gemini_api_key.clone(),
            config.gemini_model.clone(),
            Duration::from_secs(config.gemini_timeout_secs),
        )?;
        let local_principal = WorkspaceRepo::load_local_principal_or_err(&db_pool).await?;

        Ok(Self {
            config: config.clone(),
            db: db_pool.clone(),
            redis,
            document_service: DocumentService::new(db_pool.clone(), storage.clone()),
            template_service: TemplateService::new(db_pool.clone()),
            extraction_service: ExtractionService::new(
                db_pool.clone(),
                queue.clone(),
                storage,
                gemini.clone(),
            ),
            batch_service: BatchService::new(db_pool.clone(), queue),
            model_service: ModelService::new(db_pool, config.gemini_model.clone()),
            gemini,
            local_principal,
        })
    }
}
