//! Object-safe, provider-neutral extraction interface.
//!
//! Orchestration (API services and the worker) depends on [`ExtractionProvider`],
//! not on a vendor client. Concrete adapters such as Gemini live beside this
//! trait and advertise identity through [`struxio_contracts::BackendDescriptor`].

use async_trait::async_trait;
use std::sync::Arc;
use struxio_contracts::{
    BackendCapabilities, BackendCapability, BackendCompatibility, BackendDescriptor,
};

pub mod fake;

/// Shared handle stored in process-wide app state.
pub type SharedExtractionProvider = Arc<dyn ExtractionProvider>;

/// Input to a single extraction call. Bytes are borrowed; adapters copy only
/// when a backend requires it (for example base64 encoding).
#[derive(Debug, Clone, Copy)]
pub struct ExtractionRequest<'a> {
    pub bytes: &'a [u8],
    pub mime_type: &'a str,
    pub instructions: &'a str,
    pub json_schema: &'a serde_json::Value,
}

/// Token usage reported by a backend. Orchestration persists these counts
/// unchanged; this layer does not price or bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractionUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
}

/// Provider-neutral structured extraction result.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractionOutput {
    pub result: serde_json::Value,
    pub usage: ExtractionUsage,
}

/// Failures that orchestration can record without knowing the vendor.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error(
        "incompatible backend `{backend_id}` for provider `{provider_id}` (missing {missing:?})"
    )]
    Incompatible {
        provider_id: String,
        backend_id: String,
        missing: Vec<BackendCapability>,
    },
    #[error("invalid structured output: {0}")]
    StructuredOutput(String),
    #[error("extraction timed out")]
    Timeout,
    #[error("transient backend failure: {0}")]
    Transient(String),
    #[error("{0}")]
    Backend(String),
}

/// Object-safe extraction backend.
///
/// The trait is intentionally free of generics so it can be stored as
/// `Arc<dyn ExtractionProvider>` in API and worker process state.
#[async_trait]
pub trait ExtractionProvider: Send + Sync {
    /// Opaque provider/backend identity and advertised capabilities.
    fn identity(&self) -> &BackendDescriptor;

    /// Extract structured JSON for `request.json_schema`.
    async fn extract(
        &self,
        request: ExtractionRequest<'_>,
    ) -> Result<ExtractionOutput, ProviderError>;
}

/// Reject a backend that cannot satisfy contract compatibility constraints.
pub fn ensure_compatible(
    provider: &dyn ExtractionProvider,
    compatibility: &BackendCompatibility,
) -> Result<(), ProviderError> {
    let identity = provider.identity();
    if compatibility.is_satisfied_by(identity) {
        return Ok(());
    }
    Err(ProviderError::Incompatible {
        provider_id: identity.provider_id().to_string(),
        backend_id: identity.backend_id().to_string(),
        missing: compatibility.missing_capabilities(identity),
    })
}

const _: Option<&dyn ExtractionProvider> = None;

/// Default Gemini-class capability set: structured JSON, raw bytes, vision.
pub fn structured_document_capabilities() -> BackendCapabilities {
    BackendCapabilities::new()
        .with(BackendCapability::StructuredJson)
        .with(BackendCapability::RawBytes)
        .with(BackendCapability::Vision)
        .with(BackendCapability::LongContext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gemini::{GeminiClient, GEMINI_PROVIDER_ID};
    use crate::provider::fake::{FakeExtractionProvider, FAKE_BACKEND_ID, FAKE_PROVIDER_ID};
    use std::time::Duration;
    use struxio_contracts::BackendCapability;

    fn sample_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "vendor": { "type": "string" },
                "total": { "type": "number" }
            }
        })
    }

    fn sample_request<'a>(
        bytes: &'a [u8],
        mime_type: &'a str,
        instructions: &'a str,
        schema: &'a serde_json::Value,
    ) -> ExtractionRequest<'a> {
        ExtractionRequest {
            bytes,
            mime_type,
            instructions,
            json_schema: schema,
        }
    }

    #[tokio::test]
    async fn fake_provider_returns_structured_output_and_usage() {
        let provider = FakeExtractionProvider::succeeding(serde_json::json!({
            "vendor": "Acme",
            "total": 19.5
        }))
        .with_usage(11, 7);

        let boxed: SharedExtractionProvider = Arc::new(provider);
        let _: &dyn ExtractionProvider = boxed.as_ref();

        let schema = sample_schema();
        let output = boxed
            .extract(sample_request(
                b"%PDF-fake",
                "application/pdf",
                "Extract the invoice fields.",
                &schema,
            ))
            .await
            .expect("fake extract");

        assert_eq!(output.result["vendor"], "Acme");
        assert_eq!(output.result["total"], 19.5);
        assert_eq!(
            output.usage,
            ExtractionUsage {
                input_tokens: 11,
                output_tokens: 7,
            }
        );
        assert_eq!(boxed.identity().provider_id(), FAKE_PROVIDER_ID);
        assert_eq!(boxed.identity().backend_id(), FAKE_BACKEND_ID);
        assert!(boxed
            .identity()
            .capabilities()
            .supports(BackendCapability::StructuredJson));
    }

    #[tokio::test]
    async fn fake_provider_surfaces_backend_failure() {
        let provider = FakeExtractionProvider::failing("upstream unavailable");
        let schema = sample_schema();
        let err = provider
            .extract(sample_request(b"bytes", "image/png", "Extract.", &schema))
            .await
            .expect_err("fake should fail");
        assert_eq!(err, ProviderError::Backend("upstream unavailable".into()));
    }

    #[tokio::test]
    async fn fake_provider_validates_mime_type() {
        let provider = FakeExtractionProvider::succeeding(serde_json::json!({ "ok": true }));
        let schema = sample_schema();
        let err = provider
            .extract(sample_request(
                b"bytes",
                "application/octet-stream",
                "Extract.",
                &schema,
            ))
            .await
            .expect_err("unsupported mime");
        match err {
            ProviderError::UnsupportedMediaType(message) => {
                assert!(message.contains("application/octet-stream"));
            }
            other => panic!("expected unsupported media type, got {other:?}"),
        }
    }

    #[test]
    fn compatibility_rejects_missing_capabilities() {
        let provider = FakeExtractionProvider::succeeding(serde_json::json!({}));
        let required = BackendCompatibility::new()
            .requiring(BackendCapability::StructuredJson)
            .requiring(BackendCapability::SourceEvidence)
            .allowing_provider(FAKE_PROVIDER_ID);

        let err = ensure_compatible(&provider, &required).expect_err("missing evidence");
        match err {
            ProviderError::Incompatible {
                provider_id,
                missing,
                ..
            } => {
                assert_eq!(provider_id, FAKE_PROVIDER_ID);
                assert_eq!(missing, vec![BackendCapability::SourceEvidence]);
            }
            other => panic!("expected incompatible, got {other:?}"),
        }

        let json_only = BackendCompatibility::new().requiring(BackendCapability::StructuredJson);
        assert!(ensure_compatible(&provider, &json_only).is_ok());
    }

    #[test]
    fn gemini_adapter_advertises_identity_and_keeps_timeout() {
        let timeout = Duration::from_secs(45);
        let client = GeminiClient::new(
            "test-key".to_string(),
            "gemini-2.5-flash".to_string(),
            timeout,
        )
        .expect("gemini adapter");

        let provider: &dyn ExtractionProvider = &client;

        assert_eq!(provider.identity().provider_id(), GEMINI_PROVIDER_ID);
        assert_eq!(provider.identity().backend_id(), "gemini-2.5-flash");
        assert_eq!(client.request_timeout(), timeout);
        for capability in [
            BackendCapability::StructuredJson,
            BackendCapability::RawBytes,
            BackendCapability::Vision,
            BackendCapability::LongContext,
        ] {
            assert!(
                provider.identity().capabilities().supports(capability),
                "gemini should advertise {capability:?}"
            );
        }
        assert!(!provider
            .identity()
            .capabilities()
            .supports(BackendCapability::SourceEvidence));
    }

    #[tokio::test]
    async fn gemini_adapter_rejects_unsupported_mime_without_network() {
        let client = GeminiClient::new(
            "test-key".to_string(),
            "gemini-2.5-flash".to_string(),
            Duration::from_secs(1),
        )
        .expect("gemini adapter");
        let schema = sample_schema();
        let err = client
            .extract(sample_request(
                b"not-a-pdf",
                "application/msword",
                "Extract.",
                &schema,
            ))
            .await
            .expect_err("mime should fail closed");
        match err {
            ProviderError::UnsupportedMediaType(message) => {
                assert!(message.contains("application/msword"));
            }
            other => panic!("expected unsupported media type, got {other:?}"),
        }
    }
}
