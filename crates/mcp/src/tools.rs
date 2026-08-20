// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use struxio_core::services::{
    batch_service::MAX_BATCH_DOCUMENTS, extraction_service::MAX_DECODED_INLINE_BYTES,
};
use uuid::Uuid;

use crate::error::{McpError, McpResult};
use crate::limits::{MAX_FILE_NAME_BYTES, MAX_FILE_TYPE_BYTES};

/// Compact MCP tool catalog for this adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolName {
    ListTemplates,
    GetTemplate,
    Extract,
    GetExtraction,
    ExtractBatch,
    GetBatch,
}

impl ToolName {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "list_templates" => Some(Self::ListTemplates),
            "get_template" => Some(Self::GetTemplate),
            "extract" => Some(Self::Extract),
            "get_extraction" => Some(Self::GetExtraction),
            "extract_batch" => Some(Self::ExtractBatch),
            "get_batch" => Some(Self::GetBatch),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ListTemplates => "list_templates",
            Self::GetTemplate => "get_template",
            Self::Extract => "extract",
            Self::GetExtraction => "get_extraction",
            Self::ExtractBatch => "extract_batch",
            Self::GetBatch => "get_batch",
        }
    }
}

/// Source for [`extract`]: inline bytes or an existing workspace document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractSource {
    Inline {
        file_name: String,
        file_type: String,
        file_base64: String,
    },
    Document {
        document_id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractArgs {
    pub template_id: Uuid,
    pub source: ExtractSource,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractInput {
    template_id: Uuid,
    document_id: Option<Uuid>,
    file_name: Option<String>,
    file_type: Option<String>,
    file_base64: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateIdInput {
    template_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractionIdInput {
    extraction_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchIdInput {
    batch_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchInput {
    document_ids: Vec<Uuid>,
    template_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

/// JSON Schema catalog returned by `tools/list`.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            ToolName::ListTemplates,
            "List extraction templates in the authenticated workspace. Returns id, name, description, and is_system only.",
            object_schema(Map::new(), Vec::new()),
        ),
        tool(
            ToolName::GetTemplate,
            "Get one extraction template, including its JSON Schema and prompt.",
            uuid_schema("template_id"),
        ),
        tool(
            ToolName::Extract,
            "Extract structured JSON using a workspace template. Provide either an existing document_id or inline file_base64 (max 8 MiB decoded). Do not send both.",
            extract_schema(),
        ),
        tool(
            ToolName::GetExtraction,
            "Get one extraction job and its current status or result.",
            uuid_schema("extraction_id"),
        ),
        tool(
            ToolName::ExtractBatch,
            "Enqueue asynchronous extraction of up to 100 existing workspace documents with one template.",
            batch_schema(),
        ),
        tool(
            ToolName::GetBatch,
            "Get one batch job and its progress in the authenticated workspace.",
            uuid_schema("batch_id"),
        ),
    ]
}

pub fn parse_empty(arguments: Value) -> McpResult<()> {
    let _: EmptyInput = parse_value(arguments)?;
    Ok(())
}

pub fn parse_template_id(arguments: Value) -> McpResult<Uuid> {
    let parsed: TemplateIdInput = parse_value(arguments)?;
    require_id(parsed.template_id, "template_id")
}

pub fn parse_extraction_id(arguments: Value) -> McpResult<Uuid> {
    let parsed: ExtractionIdInput = parse_value(arguments)?;
    require_id(parsed.extraction_id, "extraction_id")
}

pub fn parse_batch_id(arguments: Value) -> McpResult<Uuid> {
    let parsed: BatchIdInput = parse_value(arguments)?;
    require_id(parsed.batch_id, "batch_id")
}

pub fn parse_extract(arguments: Value) -> McpResult<ExtractArgs> {
    let input: ExtractInput = parse_value(arguments)?;
    if input.template_id.is_nil() {
        return Err(McpError::invalid_arguments(
            "template_id must not be the nil UUID",
        ));
    }
    let has_inline =
        input.file_base64.is_some() || input.file_name.is_some() || input.file_type.is_some();
    match (input.document_id, has_inline) {
        (Some(_), true) => Err(McpError::invalid_arguments(
            "provide either document_id or file_base64, not both",
        )),
        (None, false) => Err(McpError::invalid_arguments(
            "provide document_id or file_base64",
        )),
        (Some(document_id), false) => {
            if document_id.is_nil() {
                return Err(McpError::invalid_arguments(
                    "document_id must not be the nil UUID",
                ));
            }
            Ok(ExtractArgs {
                template_id: input.template_id,
                source: ExtractSource::Document { document_id },
            })
        }
        (None, true) => {
            let file_name = require_present(input.file_name, "file_name")?;
            let file_type = require_present(input.file_type, "file_type")?;
            let file_base64 = require_present(input.file_base64, "file_base64")?;
            validate_inline_fields(&file_name, &file_type)?;
            Ok(ExtractArgs {
                template_id: input.template_id,
                source: ExtractSource::Inline {
                    file_name,
                    file_type,
                    file_base64,
                },
            })
        }
    }
}

pub fn parse_batch(arguments: Value) -> McpResult<(Uuid, Vec<Uuid>)> {
    let input: BatchInput = parse_value(arguments)?;
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
    if input.document_ids.iter().any(Uuid::is_nil) {
        return Err(McpError::invalid_arguments(
            "document_ids must not contain the nil UUID",
        ));
    }
    let unique: HashSet<_> = input.document_ids.iter().copied().collect();
    if unique.len() != input.document_ids.len() {
        return Err(McpError::invalid_arguments(
            "document_ids must not contain duplicates",
        ));
    }
    Ok((input.template_id, input.document_ids))
}

fn validate_inline_fields(file_name: &str, file_type: &str) -> McpResult<()> {
    if file_name.is_empty() || file_name.len() > MAX_FILE_NAME_BYTES {
        return Err(McpError::invalid_arguments(format!(
            "file_name must be between 1 and {MAX_FILE_NAME_BYTES} bytes"
        )));
    }
    if file_type.is_empty() || file_type.len() > MAX_FILE_TYPE_BYTES {
        return Err(McpError::invalid_arguments(format!(
            "file_type must be between 1 and {MAX_FILE_TYPE_BYTES} bytes"
        )));
    }
    Ok(())
}

fn parse_value<T: for<'de> Deserialize<'de>>(arguments: Value) -> McpResult<T> {
    serde_json::from_value(arguments)
        .map_err(|_| McpError::invalid_arguments("tool arguments do not match the schema"))
}

fn require_present(value: Option<String>, field: &str) -> McpResult<String> {
    value.filter(|value| !value.is_empty()).ok_or_else(|| {
        McpError::invalid_arguments(format!("{field} is required for inline extract"))
    })
}

fn require_id(id: Uuid, field: &str) -> McpResult<Uuid> {
    if id.is_nil() {
        Err(McpError::invalid_arguments(format!(
            "{field} must not be the nil UUID"
        )))
    } else {
        Ok(id)
    }
}

fn tool(name: ToolName, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name.as_str(),
        "description": description,
        "inputSchema": input_schema,
    })
}

fn uuid_schema(field: &str) -> Value {
    object_schema(
        Map::from_iter([(
            field.to_string(),
            json!({"type": "string", "format": "uuid"}),
        )]),
        vec![field.to_string()],
    )
}

fn object_schema(properties: Map<String, Value>, required: Vec<String>) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn extract_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "template_id": {"type": "string", "format": "uuid"},
            "document_id": {"type": "string", "format": "uuid"},
            "file_name": {"type": "string", "maxLength": MAX_FILE_NAME_BYTES},
            "file_type": {"type": "string", "maxLength": MAX_FILE_TYPE_BYTES},
            "file_base64": {
                "type": "string",
                "description": "Standard base64 file bytes. Mutually exclusive with document_id. Decoded size max 8 MiB."
            }
        },
        "required": ["template_id"],
        "additionalProperties": false,
        "oneOf": [
            {"required": ["document_id"]},
            {"required": ["file_name", "file_type", "file_base64"]}
        ]
    })
}

fn batch_schema() -> Value {
    object_schema(
        Map::from_iter([
            (
                "document_ids".to_string(),
                json!({
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_BATCH_DOCUMENTS,
                    "items": {"type": "string", "format": "uuid"}
                }),
            ),
            (
                "template_id".to_string(),
                json!({"type": "string", "format": "uuid"}),
            ),
        ]),
        vec!["document_ids".to_string(), "template_id".to_string()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    #[test]
    fn catalog_exposes_the_compact_rfc_aligned_surface() {
        let definitions = tool_definitions();
        let names: Vec<&str> = definitions
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "list_templates",
                "get_template",
                "extract",
                "get_extraction",
                "extract_batch",
                "get_batch"
            ]
        );
        for tool in tool_definitions() {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        }
    }

    #[test]
    fn extract_schema_requires_template_and_one_source() {
        let schema = tool_definitions()
            .into_iter()
            .find(|tool| tool["name"] == "extract")
            .unwrap();
        let required = schema["inputSchema"]["required"].as_array().unwrap();
        assert_eq!(required, &vec![json!("template_id")]);
        assert!(schema["inputSchema"]["oneOf"].is_array());
        assert_eq!(schema["inputSchema"]["oneOf"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn extract_args_accept_exactly_one_source() {
        let template_id = Uuid::from_u128(1);
        let document_id = Uuid::from_u128(2);
        let inline = parse_extract(json!({
            "template_id": template_id,
            "file_name": "invoice.pdf",
            "file_type": "pdf",
            "file_base64": STANDARD.encode(b"%PDF"),
        }))
        .unwrap();
        assert!(matches!(inline.source, ExtractSource::Inline { .. }));

        let from_doc = parse_extract(json!({
            "template_id": template_id,
            "document_id": document_id,
        }))
        .unwrap();
        assert_eq!(from_doc.source, ExtractSource::Document { document_id });

        let both = parse_extract(json!({
            "template_id": template_id,
            "document_id": document_id,
            "file_name": "invoice.pdf",
            "file_type": "pdf",
            "file_base64": STANDARD.encode(b"%PDF"),
        }))
        .unwrap_err();
        assert_eq!(both.code(), "invalid_arguments");

        let neither = parse_extract(json!({ "template_id": template_id })).unwrap_err();
        assert_eq!(neither.code(), "invalid_arguments");
    }

    #[test]
    fn extract_rejects_unknown_fields_and_nil_ids() {
        let err = parse_extract(json!({
            "template_id": Uuid::from_u128(1),
            "document_id": Uuid::from_u128(2),
            "workspace_id": Uuid::from_u128(3),
        }))
        .unwrap_err();
        assert_eq!(err.code(), "invalid_arguments");

        let err = parse_extract(json!({
            "template_id": Uuid::nil(),
            "document_id": Uuid::from_u128(2),
        }))
        .unwrap_err();
        assert!(err.message().contains("nil UUID"));
    }

    #[test]
    fn inline_size_limit_is_delegated_to_the_service() {
        let args = parse_extract(json!({
            "template_id": Uuid::from_u128(1),
            "file_name": "huge.bin",
            "file_type": "pdf",
            "file_base64": STANDARD.encode(vec![0_u8; MAX_DECODED_INLINE_BYTES + 1]),
        }))
        .expect("the service owns the decoded size limit");

        assert!(matches!(args.source, ExtractSource::Inline { .. }));
    }

    #[test]
    fn batch_schema_and_parser_enforce_bounds() {
        let schema = tool_definitions()
            .into_iter()
            .find(|tool| tool["name"] == "extract_batch")
            .unwrap();
        assert_eq!(
            schema["inputSchema"]["properties"]["document_ids"]["maxItems"],
            MAX_BATCH_DOCUMENTS
        );

        let id = Uuid::from_u128(9);
        let err = parse_batch(json!({
            "template_id": Uuid::from_u128(1),
            "document_ids": [id, id],
        }))
        .unwrap_err();
        assert_eq!(err.code(), "invalid_arguments");

        let too_many: Vec<_> = (1..=MAX_BATCH_DOCUMENTS + 1)
            .map(|value| Uuid::from_u128(value as u128))
            .collect();
        let (_, document_ids) = parse_batch(json!({
            "template_id": Uuid::from_u128(1),
            "document_ids": too_many,
        }))
        .expect("the service owns the batch size limit");
        assert_eq!(document_ids.len(), MAX_BATCH_DOCUMENTS + 1);
    }

    #[test]
    fn list_templates_rejects_unknown_arguments() {
        let err = parse_empty(json!({"limit": 10})).unwrap_err();
        assert_eq!(err.code(), "invalid_arguments");
        parse_empty(json!({})).unwrap();
    }
}
