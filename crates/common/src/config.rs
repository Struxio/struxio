use std::env;
use std::time::Duration;

pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
pub const DEFAULT_GEMINI_TIMEOUT_SECS: u64 = 120;
pub const DEFAULT_WORKER_CONCURRENCY: usize = 8;
pub const DEFAULT_WORKER_MAX_ATTEMPTS: u32 = 5;
pub const DEFAULT_WORKER_INITIAL_BACKOFF_MS: u64 = 1_000;
pub const DEFAULT_WORKER_MAX_BACKOFF_MS: u64 = 60_000;
pub const DEFAULT_WORKER_CLAIM_IDLE_BUFFER_SECS: u64 = 30;
pub const DEFAULT_PROCESSING_LEASE_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_region: String,
    pub s3_access_key_id: String,
    pub s3_secret_access_key: String,
    pub gemini_api_key: String,
    pub gemini_model: String,
    pub gemini_timeout_secs: u64,
    pub server_host: String,
    pub server_port: u16,
    pub log_level: String,
    pub cors_allowed_origins: Vec<String>,
    pub worker_concurrency: usize,
    pub worker_max_attempts: u32,
    pub worker_initial_backoff_ms: u64,
    pub worker_max_backoff_ms: u64,
    pub worker_claim_idle_secs: u64,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
        let gemini_timeout_secs = parse_timeout_secs(env::var("GEMINI_TIMEOUT_SECS").ok());
        let minimum_claim_idle_secs = minimum_claim_idle_secs(gemini_timeout_secs);
        let worker_claim_idle_secs = parse_positive_u64(
            env::var("WORKER_CLAIM_IDLE_SECS").ok(),
            minimum_claim_idle_secs,
        )
        .max(minimum_claim_idle_secs);

        Ok(Self {
            database_url: env::var("DATABASE_URL")?,
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
            s3_endpoint: env::var("S3_ENDPOINT")?,
            s3_bucket: env::var("S3_BUCKET").unwrap_or_else(|_| "struxio".to_string()),
            s3_region: env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            s3_access_key_id: env::var("S3_ACCESS_KEY_ID")?,
            s3_secret_access_key: env::var("S3_SECRET_ACCESS_KEY")?,
            gemini_api_key: env::var("GEMINI_API_KEY")?,
            gemini_model: env::var("GEMINI_MODEL")
                .unwrap_or_else(|_| DEFAULT_GEMINI_MODEL.to_string()),
            gemini_timeout_secs,
            server_host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            server_port: env::var("SERVER_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            cors_allowed_origins: env::var("CORS_ALLOWED_ORIGINS")
                .map(|s| {
                    s.split(',')
                        .map(|o| o.trim().to_string())
                        .filter(|o| !o.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            worker_concurrency: parse_positive_usize(
                env::var("WORKER_CONCURRENCY").ok(),
                DEFAULT_WORKER_CONCURRENCY,
            ),
            worker_max_attempts: parse_positive_u32(
                env::var("WORKER_MAX_ATTEMPTS").ok(),
                DEFAULT_WORKER_MAX_ATTEMPTS,
            ),
            worker_initial_backoff_ms: parse_positive_u64(
                env::var("WORKER_INITIAL_BACKOFF_MS").ok(),
                DEFAULT_WORKER_INITIAL_BACKOFF_MS,
            ),
            worker_max_backoff_ms: parse_positive_u64(
                env::var("WORKER_MAX_BACKOFF_MS").ok(),
                DEFAULT_WORKER_MAX_BACKOFF_MS,
            ),
            worker_claim_idle_secs,
        })
    }

    pub fn worker_claim_idle(&self) -> Duration {
        Duration::from_secs(self.worker_claim_idle_secs.max(1))
    }

    pub fn processing_lease(&self) -> Duration {
        Duration::from_secs(DEFAULT_PROCESSING_LEASE_SECS)
    }
}

fn parse_timeout_secs(value: Option<String>) -> u64 {
    parse_positive_u64(value, DEFAULT_GEMINI_TIMEOUT_SECS)
}

fn minimum_claim_idle_secs(gemini_timeout_secs: u64) -> u64 {
    gemini_timeout_secs
        .saturating_add(DEFAULT_WORKER_CLAIM_IDLE_BUFFER_SECS)
        .max(DEFAULT_PROCESSING_LEASE_SECS.saturating_add(DEFAULT_WORKER_CLAIM_IDLE_BUFFER_SECS))
}

fn parse_positive_usize(value: Option<String>, default: usize) -> usize {
    value
        .and_then(|value| value.parse().ok())
        .filter(|n: &usize| *n > 0)
        .unwrap_or(default)
}

fn parse_positive_u32(value: Option<String>, default: u32) -> u32 {
    value
        .and_then(|value| value.parse().ok())
        .filter(|n: &u32| *n > 0)
        .unwrap_or(default)
}

fn parse_positive_u64(value: Option<String>, default: u64) -> u64 {
    value
        .and_then(|value| value.parse().ok())
        .filter(|n: &u64| *n > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::{
        minimum_claim_idle_secs, parse_positive_usize, parse_timeout_secs, DEFAULT_GEMINI_MODEL,
        DEFAULT_GEMINI_TIMEOUT_SECS, DEFAULT_PROCESSING_LEASE_SECS,
        DEFAULT_WORKER_CLAIM_IDLE_BUFFER_SECS, DEFAULT_WORKER_CONCURRENCY,
    };

    #[test]
    fn uses_flash_as_the_default_model() {
        assert_eq!(DEFAULT_GEMINI_MODEL, "gemini-2.5-flash");
    }

    #[test]
    fn uses_safe_timeout_for_missing_or_invalid_values() {
        assert_eq!(parse_timeout_secs(None), DEFAULT_GEMINI_TIMEOUT_SECS);
        assert_eq!(
            parse_timeout_secs(Some("0".to_string())),
            DEFAULT_GEMINI_TIMEOUT_SECS
        );
        assert_eq!(
            parse_timeout_secs(Some("not-a-duration".to_string())),
            DEFAULT_GEMINI_TIMEOUT_SECS
        );
        assert_eq!(parse_timeout_secs(Some("45".to_string())), 45);
    }

    #[test]
    fn worker_concurrency_rejects_zero() {
        assert_eq!(
            parse_positive_usize(Some("0".to_string()), DEFAULT_WORKER_CONCURRENCY),
            DEFAULT_WORKER_CONCURRENCY
        );
        assert_eq!(
            parse_positive_usize(Some("16".to_string()), DEFAULT_WORKER_CONCURRENCY),
            16
        );
    }

    #[test]
    fn claim_idle_is_longer_than_timeout_and_processing_lease() {
        let minimum = minimum_claim_idle_secs(120);
        assert!(minimum > 120);
        assert!(minimum > DEFAULT_PROCESSING_LEASE_SECS);
        assert_eq!(minimum, 120 + DEFAULT_WORKER_CLAIM_IDLE_BUFFER_SECS);
    }
}
