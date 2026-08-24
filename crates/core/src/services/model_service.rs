use sqlx::PgPool;
use struxio_common::{models::AiModel, AppError};
use struxio_db::repositories::ai_models::AiModelRepo;

/// Lightweight service for looking up AI model metadata from the database.
/// The model to use is configured via the `GEMINI_MODEL` environment variable
/// and seeded by the migration. On self-hosted deployments there is always
/// exactly one active model.
#[derive(Clone)]
pub struct ModelService {
    db: PgPool,
    default_model_id: String,
}

impl ModelService {
    pub fn new(db: PgPool, default_model_id: String) -> Self {
        Self {
            db,
            default_model_id,
        }
    }

    pub async fn get_default_model(&self) -> Result<AiModel, AppError> {
        AiModelRepo::find_by_id(&self.db, &self.default_model_id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?
            .ok_or_else(|| {
                AppError::Internal(format!(
                    "AI model '{}' not found in database. Check your GEMINI_MODEL env var \
                     and migration seed data.",
                    self.default_model_id
                ))
            })
    }
}
