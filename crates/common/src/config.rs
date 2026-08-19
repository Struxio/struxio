use std::env;

pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
pub const DEFAULT_GEMINI_TIMEOUT_SECS: u64 = 120;
pub const DEFAULT_WORKER_CONCURRENCY: usize = 4;
pub const DEFAULT_QUEUE_VISIBILITY_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_QUEUE_MAX_ATTEMPTS: u32 = 5;
pub const DEFAULT_QUEUE_RETRY_INITIAL_SECS: u64 = 5;
pub const DEFAULT_QUEUE_RETRY_MAX_SECS: u64 = 300;

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
    pub worker_concurrency: usize,
    pub queue_visibility_timeout_secs: u64,
    pub queue_max_attempts: u32,
    pub queue_retry_initial_secs: u64,
    pub queue_retry_max_secs: u64,
    pub server_host: String,
    pub server_port: u16,
    pub log_level: String,
    pub cors_allowed_origins: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
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
            gemini_timeout_secs: parse_timeout_secs(env::var("GEMINI_TIMEOUT_SECS").ok()),
            worker_concurrency: parse_usize(
                env::var("WORKER_CONCURRENCY").ok(),
                DEFAULT_WORKER_CONCURRENCY,
            ),
            queue_visibility_timeout_secs: parse_u64(
                env::var("QUEUE_VISIBILITY_TIMEOUT_SECS").ok(),
                DEFAULT_QUEUE_VISIBILITY_TIMEOUT_SECS,
            ),
            queue_max_attempts: parse_u32(
                env::var("QUEUE_MAX_ATTEMPTS").ok(),
                DEFAULT_QUEUE_MAX_ATTEMPTS,
            ),
            queue_retry_initial_secs: parse_u64(
                env::var("QUEUE_RETRY_INITIAL_SECS").ok(),
                DEFAULT_QUEUE_RETRY_INITIAL_SECS,
            ),
            queue_retry_max_secs: parse_u64(
                env::var("QUEUE_RETRY_MAX_SECS").ok(),
                DEFAULT_QUEUE_RETRY_MAX_SECS,
            ),
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
        })
    }
}

fn parse_timeout_secs(value: Option<String>) -> u64 {
    parse_u64(value, DEFAULT_GEMINI_TIMEOUT_SECS)
}

fn parse_u64(value: Option<String>, default: u64) -> u64 {
    value
        .and_then(|value| value.parse().ok())
        .filter(|seconds: &u64| *seconds > 0)
        .unwrap_or(default)
}

fn parse_u32(value: Option<String>, default: u32) -> u32 {
    value
        .and_then(|value| value.parse().ok())
        .filter(|value: &u32| *value > 0)
        .unwrap_or(default)
}

fn parse_usize(value: Option<String>, default: usize) -> usize {
    value
        .and_then(|value| value.parse().ok())
        .filter(|value: &usize| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_timeout_secs, parse_u32, parse_u64, parse_usize, DEFAULT_GEMINI_MODEL,
        DEFAULT_GEMINI_TIMEOUT_SECS,
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
    fn queue_settings_reject_zero_and_invalid_values() {
        assert_eq!(parse_usize(Some("0".to_string()), 4), 4);
        assert_eq!(parse_usize(Some("3".to_string()), 4), 3);
        assert_eq!(parse_u32(Some("nope".to_string()), 5), 5);
        assert_eq!(parse_u64(Some("30".to_string()), 5), 30);
    }
}
