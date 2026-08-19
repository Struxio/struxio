use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use uuid::Uuid;

use crate::principal::WorkspaceId;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl Display for ExtractionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractionStatus::Pending => write!(f, "pending"),
            ExtractionStatus::Processing => write!(f, "processing"),
            ExtractionStatus::Completed => write!(f, "completed"),
            ExtractionStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Pending,
    Processing,
    Completed,
    PartiallyCompleted,
    Failed,
}

impl Display for BatchStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatchStatus::Pending => write!(f, "pending"),
            BatchStatus::Processing => write!(f, "processing"),
            BatchStatus::Completed => write!(f, "completed"),
            BatchStatus::PartiallyCompleted => write!(f, "partially_completed"),
            BatchStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub md5_hash: String,
    pub file_name: String,
    pub file_type: String,
    pub s3_key: String,
    pub size_bytes: i64,
    pub page_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionTemplate {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub description: Option<String>,
    pub json_schema: serde_json::Value,
    pub prompt_template: String,
    pub is_system: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extraction {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub document_id: Uuid,
    pub template_id: Uuid,
    pub batch_job_id: Option<Uuid>,
    pub status: String,
    pub result: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub model_id: Option<String>,
    pub credits_charged: i32,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub processing_time_ms: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJob {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub template_id: Uuid,
    pub status: String,
    pub total_documents: i32,
    pub completed_documents: i32,
    pub failed_documents: i32,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckDocumentRequest {
    pub md5_hash: String,
    pub file_name: String,
    pub file_type: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckDocumentResponse {
    pub exists: bool,
    pub document: Option<Document>,
    pub upload_url: Option<String>,
    pub s3_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmUploadRequest {
    pub s3_key: String,
    pub md5_hash: String,
    pub file_name: String,
    pub file_type: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateExtractionRequest {
    pub document_id: Uuid,
    pub template_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineExtractionRequest {
    pub file_name: String,
    pub file_type: String,
    /// Standard base64-encoded file contents.
    pub file_base64: String,
    pub template_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBatchRequest {
    pub document_ids: Vec<Uuid>,
    pub template_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTemplateRequest {
    pub name: String,
    pub description: Option<String>,
    pub json_schema: serde_json::Value,
    pub prompt_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTemplateRequest {
    pub name: String,
    pub description: Option<String>,
    pub json_schema: serde_json::Value,
    pub prompt_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    pub id: String,
    pub display_name: String,
    pub credit_cost_per_page: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}
