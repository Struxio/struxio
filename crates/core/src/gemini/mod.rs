use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
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
pub struct GeminiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

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

impl GeminiClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
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

        let resp = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?;

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
