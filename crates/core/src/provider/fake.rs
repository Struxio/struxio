//! In-memory extraction backend for tests. Never used as a production adapter.

use async_trait::async_trait;
use struxio_common::mime::normalize_mime_type;
use struxio_contracts::{BackendCapabilities, BackendCapability, BackendDescriptor};

use super::{
    ExtractionOutput, ExtractionProvider, ExtractionRequest, ExtractionUsage, ProviderError,
};

/// Opaque identity used by the canned test double.
pub const FAKE_PROVIDER_ID: &str = "fake";
/// Backend id for the canned test double.
pub const FAKE_BACKEND_ID: &str = "canned";

/// Deterministic [`ExtractionProvider`] that returns canned JSON and usage.
#[derive(Debug, Clone)]
pub struct FakeExtractionProvider {
    identity: BackendDescriptor,
    result: serde_json::Value,
    usage: ExtractionUsage,
    failure: Option<ProviderError>,
}

impl FakeExtractionProvider {
    /// Succeeding double with structured JSON and zero usage.
    pub fn succeeding(result: serde_json::Value) -> Self {
        Self {
            identity: default_identity(),
            result,
            usage: ExtractionUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
            failure: None,
        }
    }

    /// Failing double that always returns [`ProviderError::Backend`].
    pub fn failing(message: impl Into<String>) -> Self {
        Self {
            identity: default_identity(),
            result: serde_json::Value::Null,
            usage: ExtractionUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
            failure: Some(ProviderError::Backend(message.into())),
        }
    }

    /// Override advertised identity/capabilities.
    pub fn with_identity(mut self, identity: BackendDescriptor) -> Self {
        self.identity = identity;
        self
    }

    /// Set token usage returned on success.
    pub fn with_usage(mut self, input_tokens: i32, output_tokens: i32) -> Self {
        self.usage = ExtractionUsage {
            input_tokens,
            output_tokens,
        };
        self
    }
}

fn default_identity() -> BackendDescriptor {
    BackendDescriptor::new(
        FAKE_PROVIDER_ID,
        FAKE_BACKEND_ID,
        BackendCapabilities::new()
            .with(BackendCapability::StructuredJson)
            .with(BackendCapability::RawBytes),
    )
}

#[async_trait]
impl ExtractionProvider for FakeExtractionProvider {
    fn identity(&self) -> &BackendDescriptor {
        &self.identity
    }

    async fn extract(
        &self,
        request: ExtractionRequest<'_>,
    ) -> Result<ExtractionOutput, ProviderError> {
        normalize_mime_type(request.mime_type)
            .map_err(|error| ProviderError::UnsupportedMediaType(error.to_string()))?;

        if let Some(failure) = self.failure.clone() {
            return Err(failure);
        }

        Ok(ExtractionOutput {
            result: self.result.clone(),
            usage: self.usage,
        })
    }
}
