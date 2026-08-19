use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use struxio_contracts::{BackendCapabilities, BackendCapability, BackendDescriptor};

use crate::backend::{
    ExtractionBackend, ExtractionBackendError, ExtractionInput, ExtractionOutput, TokenUsage,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiResponse {
    pub result: serde_json::Value,
    pub input_tokens: i32,
    pub output_tokens: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum GeminiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {0}")]
    Api(String),
    #[error("Parse error: {0}")]
    Parse(String),
}

#[derive(Clone)]
pub struct GeminiBackend {
    api_key: String,
    model: String,
    client: reqwest::Client,
    request_timeout: Duration,
    descriptor: BackendDescriptor,
}

/// Compatibility name for callers that used the pre-provider-boundary API.
pub type GeminiClient = GeminiBackend;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<Content>,
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Part {
    Text { text: String },
    InlineData { inline_data: InlineData },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    response_mime_type: String,
    response_schema: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiApiResponse {
    candidates: Option<Vec<Candidate>>,
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize)]
struct Candidate {
    content: CandidateContent,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Vec<CandidatePart>,
}

#[derive(Deserialize)]
struct CandidatePart {
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageMetadata {
    prompt_token_count: Option<i32>,
    candidates_token_count: Option<i32>,
}

impl GeminiBackend {
    pub fn new(
        api_key: String,
        model: String,
        request_timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .timeout(request_timeout)
            .build()?;

        Ok(Self {
            api_key,
            model,
            client,
            request_timeout,
            descriptor: BackendDescriptor::new(
                "gemini",
                model.clone(),
                BackendCapabilities::new()
                    .with(BackendCapability::RawBytes)
                    .with(BackendCapability::StructuredJson)
                    .with(BackendCapability::Vision),
            ),
        })
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub async fn extract(
        &self,
        file_bytes: &[u8],
        mime_type: &str,
        prompt_template: &str,
        json_schema: &serde_json::Value,
    ) -> Result<GeminiResponse, GeminiError> {
        let base64_data = BASE64.encode(file_bytes);

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![
                    Part::Text {
                        text: prompt_template.to_string(),
                    },
                    Part::InlineData {
                        inline_data: InlineData {
                            mime_type: mime_type.to_string(),
                            data: base64_data,
                        },
                    },
                ],
            }],
            generation_config: GenerationConfig {
                response_mime_type: "application/json".to_string(),
                response_schema: json_schema.clone(),
            },
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let resp = self.client.post(&url).json(&request).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(GeminiError::Api(format!(
                "Gemini API returned {}: {}",
                status, body
            )));
        }

        let api_response: GeminiApiResponse = resp
            .json()
            .await
            .map_err(|e| GeminiError::Parse(e.to_string()))?;

        let text = api_response
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.content.parts.first())
            .and_then(|p| p.text.as_ref())
            .ok_or_else(|| GeminiError::Parse("No text in response candidates".to_string()))?;

        let result: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| GeminiError::Parse(format!("Invalid JSON in response: {}", e)))?;

        let input_tokens = api_response
            .usage_metadata
            .as_ref()
            .and_then(|m| m.prompt_token_count)
            .unwrap_or(0);

        let output_tokens = api_response
            .usage_metadata
            .as_ref()
            .and_then(|m| m.candidates_token_count)
            .unwrap_or(0);

        Ok(GeminiResponse {
            result,
            input_tokens,
            output_tokens,
        })
    }
}

impl GeminiBackend {
    pub fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }
}

impl From<GeminiError> for ExtractionBackendError {
    fn from(error: GeminiError) -> Self {
        match error {
            GeminiError::Http(error) => Self::Request(error.to_string()),
            GeminiError::Api(error) => Self::Api(error),
            GeminiError::Parse(error) => Self::Parse(error),
        }
    }
}

#[async_trait::async_trait]
impl ExtractionBackend for GeminiBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        self.descriptor()
    }

    async fn extract(
        &self,
        input: ExtractionInput,
    ) -> Result<ExtractionOutput, ExtractionBackendError> {
        let response = self
            .extract(
                &input.file_bytes,
                &input.mime_type,
                &input.prompt,
                &input.json_schema,
            )
            .await
            .map_err(ExtractionBackendError::from)?;

        Ok(ExtractionOutput::new(
            response.result,
            TokenUsage::new(response.input_tokens, response.output_tokens),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::GeminiClient;
    use std::time::Duration;

    #[test]
    fn builds_client_with_configured_request_timeout() {
        let timeout = Duration::from_secs(45);
        let client = GeminiClient::new(
            "test-key".to_string(),
            "gemini-2.5-flash".to_string(),
            timeout,
        )
        .expect("test client should build");

        assert_eq!(client.request_timeout(), timeout);
    }
}
