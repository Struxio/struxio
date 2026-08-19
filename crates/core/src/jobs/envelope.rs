// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use struxio_common::WorkspaceId;
use uuid::Uuid;

use super::error::JobError;

pub const READY_STREAM: &str = "extractions:queue";
pub const DLQ_STREAM: &str = "extractions:dlq";
pub const DELAYED_ZSET: &str = "extractions:delayed";
pub const CONSUMER_GROUP: &str = "workers";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobEnvelope {
    pub extraction_id: Uuid,
    pub document_id: Uuid,
    pub template_id: Uuid,
    pub workspace_id: Uuid,
    pub batch_job_id: Option<Uuid>,
}

impl JobEnvelope {
    pub fn workspace(&self) -> Result<WorkspaceId, JobError> {
        WorkspaceId::new(self.workspace_id)
            .map_err(|_| JobError::permanent("workspace_id must not be the nil UUID"))
    }

    pub fn to_fields(&self) -> Vec<(String, String)> {
        let mut fields = vec![
            ("extraction_id".to_string(), self.extraction_id.to_string()),
            ("document_id".to_string(), self.document_id.to_string()),
            ("template_id".to_string(), self.template_id.to_string()),
            ("workspace_id".to_string(), self.workspace_id.to_string()),
        ];
        if let Some(batch_job_id) = self.batch_job_id {
            fields.push(("batch_job_id".to_string(), batch_job_id.to_string()));
        }
        fields
    }

    pub fn from_fields(fields: &HashMap<String, String>) -> Result<Self, JobError> {
        let extraction_id = required_uuid(fields, "extraction_id")?;
        let document_id = required_uuid(fields, "document_id")?;
        let template_id = required_uuid(fields, "template_id")?;
        let workspace_id = required_uuid(fields, "workspace_id")?;
        if workspace_id.is_nil() {
            return Err(JobError::permanent("workspace_id must not be the nil UUID"));
        }
        let batch_job_id = match fields.get("batch_job_id") {
            Some(value) if !value.is_empty() => Some(
                Uuid::parse_str(value)
                    .map_err(|_| JobError::permanent("batch_job_id is not a UUID"))?,
            ),
            _ => None,
        };
        Ok(Self {
            extraction_id,
            document_id,
            template_id,
            workspace_id,
            batch_job_id,
        })
    }

    pub fn to_delayed_payload(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_delayed_payload(payload: &str) -> Result<Self, JobError> {
        let envelope: Self = serde_json::from_str(payload)
            .map_err(|e| JobError::permanent(format!("delayed payload is not valid JSON: {e}")))?;
        if envelope.workspace_id.is_nil() {
            return Err(JobError::permanent("workspace_id must not be the nil UUID"));
        }
        Ok(envelope)
    }
}

fn required_uuid(fields: &HashMap<String, String>, key: &str) -> Result<Uuid, JobError> {
    let value = fields
        .get(key)
        .ok_or_else(|| JobError::permanent(format!("missing field {key}")))?;
    Uuid::parse_str(value).map_err(|_| JobError::permanent(format!("{key} is not a UUID")))
}
