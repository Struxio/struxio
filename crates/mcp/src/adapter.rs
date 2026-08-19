use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use struxio_common::models::{
    BatchJob, CreateBatchRequest, Extraction, ExtractionTemplate, InlineExtractionRequest,
};
use struxio_common::{AppError, PrincipalContext, Scope};
use struxio_core::queue::QueueProducer;
use struxio_core::services::{
    batch_service::BatchService, extraction_service::ExtractionService,
    model_service::ModelService, template_service::TemplateService,
};
use thiserror::Error;
use uuid::Uuid;

/// Maximum decoded size accepted by the inline tool.
pub const MAX_INLINE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum number of documents accepted by one batch submission.
pub const MAX_BATCH_DOCUMENTS: usize = 100;
/// Maximum length of a single JSON-RPC message, including its newline.
pub const MAX_MESSAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_FILE_NAME_BYTES: usize = 255;
const MAX_FILE_TYPE_BYTES: usize = 64;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum McpError {
    #[error("{message}")]
    Request { code: &'static str, message: String },
    #[error("{message}")]
    App { code: &'static str, message: String },
}

impl McpError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Request { code, .. } | Self::App { code, .. } => code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Request { message, .. } | Self::App { message, .. } => message,
        }
    }

    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::Request {
            code: "invalid_arguments",
            message: message.into(),
        }
    }

    pub fn from_app(error: AppError) -> Self {
        let (code, message) = match error {
            AppError::Auth(_) => ("unauthorized", "authentication failed".to_string()),
            AppError::Forbidden(_) => ("forbidden", "insufficient scope".to_string()),
            AppError::Validation(message) => ("invalid_arguments", message),
            AppError::NotFound(_) => ("not_found", "resource not found".to_string()),
            AppError::RateLimit(_) => ("rate_limited", "request rate limit exceeded".to_string()),
            AppError::Duplicate(_) => ("conflict", "resource already exists".to_string()),
            AppError::Database(_) => ("internal_error", "database operation failed".to_string()),
            AppError::ExternalService(_) => {
                ("upstream_error", "upstream service failed".to_string())
            }
            AppError::Internal(_) => ("internal_error", "internal server error".to_string()),
        };
        Self::App { code, message }
    }
}

pub type McpResult<T> = Result<T, McpError>;

/// Arguments for the inline extraction tool.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InlineInput {
    pub file_name: String,
    pub file_type: String,
    pub file_base64: String,
    pub template_id: Uuid,
}

/// Arguments for the extraction status tool.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionStatusInput {
    pub extraction_id: Uuid,
}

/// Arguments for the batch submission tool.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchInput {
    pub document_ids: Vec<Uuid>,
    pub template_id: Uuid,
}

/// Arguments for the batch status tool.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchStatusInput {
    pub batch_id: Uuid,
}

/// The deliberately small capability boundary used by the protocol server.
///
/// Implementations own the business services and the authenticated principal.
/// The protocol layer only parses arguments, enforces transport limits, and
/// serializes results.
#[async_trait]
pub trait McpBackend: Send + Sync {
    async fn list_templates(&self) -> McpResult<Vec<ExtractionTemplate>>;
    async fn extract_inline(&self, input: InlineInput) -> McpResult<Extraction>;
    async fn extraction_status(&self, input: ExtractionStatusInput) -> McpResult<Extraction>;
    async fn submit_batch(&self, input: BatchInput) -> McpResult<BatchJob>;
    async fn batch_status(&self, input: BatchStatusInput) -> McpResult<BatchJob>;
}

/// Thin MCP adapter over the existing application services.
///
/// It deliberately contains no repository calls, provider logic, or queue
/// policy. The supplied context is the authorization and tenancy boundary for
/// every service call; there is no nil or implicit workspace fallback.
#[derive(Clone)]
pub struct ServiceAdapter<Q: QueueProducer> {
    template_service: TemplateService,
    extraction_service: ExtractionService<Q>,
    batch_service: BatchService<Q>,
    model_service: ModelService,
    principal: PrincipalContext,
}

impl<Q: QueueProducer> ServiceAdapter<Q> {
    pub fn new(
        template_service: TemplateService,
        extraction_service: ExtractionService<Q>,
        batch_service: BatchService<Q>,
        model_service: ModelService,
        principal: PrincipalContext,
    ) -> Self {
        Self {
            template_service,
            extraction_service,
            batch_service,
            model_service,
            principal,
        }
    }

    pub fn principal(&self) -> &PrincipalContext {
        &self.principal
    }

    fn require_scope(&self, scope: Scope) -> McpResult<()> {
        self.principal
            .require_scope(scope)
            .map_err(McpError::from_app)
    }

    fn validate_inline(input: &InlineInput) -> McpResult<()> {
        if input.file_name.is_empty() || input.file_name.len() > MAX_FILE_NAME_BYTES {
            return Err(McpError::invalid_arguments(format!(
                "file_name must be between 1 and {MAX_FILE_NAME_BYTES} bytes"
            )));
        }
        if input.file_type.is_empty() || input.file_type.len() > MAX_FILE_TYPE_BYTES {
            return Err(McpError::invalid_arguments(format!(
                "file_type must be between 1 and {MAX_FILE_TYPE_BYTES} bytes"
            )));
        }
        if input.template_id.is_nil() {
            return Err(McpError::invalid_arguments(
                "template_id must not be the nil UUID",
            ));
        }

        let decoded = STANDARD
            .decode(&input.file_base64)
            .map_err(|_| McpError::invalid_arguments("file_base64 must be standard base64"))?;
        if decoded.is_empty() {
            return Err(McpError::invalid_arguments(
                "file_base64 must contain at least one byte",
            ));
        }
        if decoded.len() > MAX_INLINE_BYTES {
            return Err(McpError::Request {
                code: "input_too_large",
                message: format!("decoded inline input exceeds {MAX_INLINE_BYTES} bytes"),
            });
        }
        Ok(())
    }

    fn validate_batch(input: &BatchInput) -> McpResult<()> {
        if input.template_id.is_nil() {
            return Err(McpError::invalid_arguments(
                "template_id must not be the nil UUID",
            ));
        }
        if input.document_ids.is_empty() {
            return Err(McpError::invalid_arguments(
                "document_ids must contain at least one UUID",
            ));
        }
        if input.document_ids.len() > MAX_BATCH_DOCUMENTS {
            return Err(McpError::Request {
                code: "input_too_large",
                message: format!(
                    "document_ids cannot contain more than {MAX_BATCH_DOCUMENTS} documents"
                ),
            });
        }
        if input.document_ids.iter().any(Uuid::is_nil) {
            return Err(McpError::invalid_arguments(
                "document_ids must not contain the nil UUID",
            ));
        }
        let unique: HashSet<_> = input.document_ids.iter().collect();
        if unique.len() != input.document_ids.len() {
            return Err(McpError::invalid_arguments(
                "document_ids must not contain duplicates",
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl<Q: QueueProducer> McpBackend for ServiceAdapter<Q> {
    async fn list_templates(&self) -> McpResult<Vec<ExtractionTemplate>> {
        self.require_scope(Scope::TemplatesRead)?;
        self.template_service
            .list(&self.principal)
            .await
            .map_err(McpError::from_app)
    }

    async fn extract_inline(&self, input: InlineInput) -> McpResult<Extraction> {
        self.require_scope(Scope::ExtractionsCreate)?;
        Self::validate_inline(&input)?;
        let model = self
            .model_service
            .get_default_model()
            .await
            .map_err(McpError::from_app)?;
        let request = InlineExtractionRequest {
            file_name: input.file_name,
            file_type: input.file_type,
            file_base64: input.file_base64,
            template_id: input.template_id,
        };
        self.extraction_service
            .create_inline(&self.principal, &request, &model.id)
            .await
            .map_err(McpError::from_app)
    }

    async fn extraction_status(&self, input: ExtractionStatusInput) -> McpResult<Extraction> {
        self.require_scope(Scope::ExtractionsRead)?;
        if input.extraction_id.is_nil() {
            return Err(McpError::invalid_arguments(
                "extraction_id must not be the nil UUID",
            ));
        }
        self.extraction_service
            .get(&self.principal, input.extraction_id)
            .await
            .map_err(McpError::from_app)
    }

    async fn submit_batch(&self, input: BatchInput) -> McpResult<BatchJob> {
        self.require_scope(Scope::BatchesCreate)?;
        Self::validate_batch(&input)?;
        let model = self
            .model_service
            .get_default_model()
            .await
            .map_err(McpError::from_app)?;
        let request = CreateBatchRequest {
            document_ids: input.document_ids,
            template_id: input.template_id,
        };
        self.batch_service
            .create(&self.principal, &request, &model.id)
            .await
            .map_err(McpError::from_app)
    }

    async fn batch_status(&self, input: BatchStatusInput) -> McpResult<BatchJob> {
        self.require_scope(Scope::BatchesRead)?;
        if input.batch_id.is_nil() {
            return Err(McpError::invalid_arguments(
                "batch_id must not be the nil UUID",
            ));
        }
        self.batch_service
            .get(&self.principal, input.batch_id)
            .await
            .map_err(McpError::from_app)
    }
}

pub fn parse_input<T: for<'de> Deserialize<'de>>(params: Option<Value>) -> McpResult<T> {
    let params = params.unwrap_or_else(|| Value::Object(Default::default()));
    serde_json::from_value(params)
        .map_err(|_| McpError::invalid_arguments("tool arguments do not match the schema"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_validation_rejects_oversized_decoded_input() {
        let input = InlineInput {
            file_name: "document.pdf".to_string(),
            file_type: "pdf".to_string(),
            file_base64: STANDARD.encode(vec![0_u8; MAX_INLINE_BYTES + 1]),
            template_id: Uuid::new_v4(),
        };

        assert_eq!(
            ServiceAdapter::<TestQueue>::validate_inline(&input),
            Err(McpError::Request {
                code: "input_too_large",
                message: format!("decoded inline input exceeds {MAX_INLINE_BYTES} bytes"),
            })
        );
    }

    #[test]
    fn batch_validation_rejects_duplicates_and_nil_ids() {
        let id = Uuid::new_v4();
        let duplicate = BatchInput {
            document_ids: vec![id, id],
            template_id: Uuid::new_v4(),
        };
        assert!(matches!(
            ServiceAdapter::<TestQueue>::validate_batch(&duplicate),
            Err(McpError::Request {
                code: "invalid_arguments",
                ..
            })
        ));

        let nil = BatchInput {
            document_ids: vec![Uuid::nil()],
            template_id: Uuid::new_v4(),
        };
        assert!(matches!(
            ServiceAdapter::<TestQueue>::validate_batch(&nil),
            Err(McpError::Request {
                code: "invalid_arguments",
                ..
            })
        ));
    }

    #[test]
    fn app_errors_use_stable_public_codes_and_messages() {
        assert_eq!(
            McpError::from_app(AppError::Database("password=secret".to_string)),
            McpError::App {
                code: "internal_error",
                message: "database operation failed".to_string(),
            }
        );
        assert_eq!(
            McpError::from_app(AppError::NotFound("secret-id".to_string())),
            McpError::App {
                code: "not_found",
                message: "resource not found".to_string(),
            }
        );
    }

    // Validation tests do not construct the service graph or touch a database.
    #[derive(Clone)]
    struct TestQueue;

    #[async_trait]
    impl QueueProducer for TestQueue {
        async fn enqueue_extraction(
            &self,
            _extraction_id: Uuid,
            _document_id: Uuid,
            _template_id: Uuid,
            _workspace_id: struxio_common::WorkspaceId,
            _batch_job_id: Option<Uuid>,
        ) -> Result<(), struxio_core::queue::QueueError> {
            Ok(())
        }
    }
}
