use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("External service error: {0}")]
    ExternalService(String),
    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),
    #[error("Duplicate resource: {0}")]
    Duplicate(String),
    #[error("Internal error: {0}")]
    Internal(String),
}
