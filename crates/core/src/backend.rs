use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use struxio_contracts::BackendDescriptor;
use thiserror::Error;

/// The provider-neutral input to an extraction backend.
///
/// Bytes and the normalized MIME type are intentionally kept together so each
/// adapter receives the same document representation. `prompt` and
/// `json_schema` retain the existing Struxio API semantics without exposing
/// HTTP, database, or vendor-specific types at this boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractionInput {
    pub file_bytes: Vec<u8>,
    pub mime_type: String,
    pub prompt: String,
    pub json_schema: Value,
}

impl ExtractionInput {
    pub fn new(
        file_bytes: Vec<u8>,
        mime_type: impl Into<String>,
        prompt: impl Into<String>,
        json_schema: Value,
    ) -> Self {
        Self {
            file_bytes,
            mime_type: mime_type.into(),
            prompt: prompt.into(),
            json_schema,
        }
    }
}

/// Token accounting returned by an extraction backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
}

impl TokenUsage {
    pub const fn new(input_tokens: i32, output_tokens: i32) -> Self {
        Self {
            input_tokens,
            output_tokens,
        }
    }
}

/// Provider-neutral successful extraction output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractionOutput {
    pub result: Value,
    pub usage: TokenUsage,
}

impl ExtractionOutput {
    pub fn new(result: Value, usage: TokenUsage) -> Self {
        Self { result, usage }
    }
}

/// Errors that can cross the extraction backend boundary.
///
/// Adapters should convert transport and vendor response details into these
/// categories. This keeps services and workers independent of reqwest and
/// provider SDK error types.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ExtractionBackendError {
    #[error("backend request failed: {0}")]
    Request(String),
    #[error("backend API error: {0}")]
    Api(String),
    #[error("backend response parse failed: {0}")]
    Parse(String),
    #[error("backend capability error: {0}")]
    Capability(String),
}

/// A provider-neutral extraction backend.
#[async_trait]
pub trait ExtractionBackend: Send + Sync {
    /// Opaque provider/backend identifiers and capabilities used by contracts.
    fn descriptor(&self) -> &BackendDescriptor;

    async fn extract(
        &self,
        input: ExtractionInput,
    ) -> Result<ExtractionOutput, ExtractionBackendError>;
}

impl fmt::Display for TokenUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "input_tokens={}, output_tokens={}",
            self.input_tokens, self.output_tokens
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use struxio_contracts::{BackendCapabilities, BackendCapability};

    #[derive(Clone)]
    struct FakeBackend {
        descriptor: BackendDescriptor,
    }

    #[async_trait]
    impl ExtractionBackend for FakeBackend {
        fn descriptor(&self) -> &BackendDescriptor {
            &self.descriptor
        }

        async fn extract(
            &self,
            input: ExtractionInput,
        ) -> Result<ExtractionOutput, ExtractionBackendError> {
            Ok(ExtractionOutput::new(
                serde_json::json!({
                    "bytes": input.file_bytes.len(),
                    "mime_type": input.mime_type,
                    "prompt": input.prompt,
                    "schema": input.json_schema,
                }),
                TokenUsage::new(3, 2),
            ))
        }
    }

    #[tokio::test]
    async fn fake_backend_is_deterministic_and_provider_neutral() {
        let backend = FakeBackend {
            descriptor: BackendDescriptor::new(
                "fake",
                "deterministic",
                BackendCapabilities::new()
                    .with(BackendCapability::RawBytes)
                    .with(BackendCapability::StructuredJson),
            ),
        };
        let input = ExtractionInput::new(
            vec![1, 2, 3],
            "application/pdf",
            "Extract fields",
            serde_json::json!({"type": "object"}),
        );

        let first = backend.extract(input.clone()).await.unwrap();
        let second = backend.extract(input).await.unwrap();

        assert_eq!(first, second);
        assert_eq!(backend.descriptor().provider_id(), "fake");
        assert!(backend
            .descriptor()
            .capabilities()
            .supports(BackendCapability::StructuredJson));
    }
}
