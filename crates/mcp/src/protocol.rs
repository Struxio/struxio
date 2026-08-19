use crate::adapter::{
    parse_input, BatchInput, BatchStatusInput, ExtractionStatusInput, InlineInput, McpBackend,
    McpError, McpResult, MAX_MESSAGE_BYTES,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt};

const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorDetail<'a>,
}

#[derive(Debug, Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: &'a str,
}

#[derive(Debug, Serialize)]
struct ToolCallResult {
    content: [ToolContent; 1],
    #[serde(rename = "isError")]
    is_error: bool,
}

#[derive(Debug, Serialize)]
struct ToolContent {
    #[serde(rename = "type")]
    content_type: &'static str,
    text: String,
}

#[derive(Debug, Serialize)]
struct Tool {
    name: &'static str,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Debug, Serialize)]
struct ToolsList {
    tools: Vec<Tool>,
}

#[derive(Debug, Serialize)]
struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    protocol_version: &'static str,
    capabilities: Map<String, Value>,
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
}

#[derive(Debug, Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug)]
enum ReadMessage {
    Eof,
    Line(Vec<u8>),
    TooLarge,
}

/// MCP stdio JSON-RPC server. Stdout is reserved for protocol messages.
pub struct McpServer<B> {
    backend: B,
    max_message_bytes: usize,
}

impl<B> McpServer<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            max_message_bytes: MAX_MESSAGE_BYTES,
        }
    }

    pub fn with_max_message_bytes(mut self, max_message_bytes: usize) -> Self {
        self.max_message_bytes = max_message_bytes.max(1);
        self
    }
}

impl<B: McpBackend> McpServer<B> {
    pub async fn handle_json(&self, input: &[u8]) -> Option<Vec<u8>> {
        match serde_json::from_slice::<JsonRpcRequest>(input) {
            Ok(request) => self.handle_request(request).await,
            Err(_) => Some(self.protocol_error(None, -32700, "parse error")),
        }
    }

    async fn handle_request(&self, request: JsonRpcRequest) -> Option<Vec<u8>> {
        if request.jsonrpc != "2.0" {
            return Some(self.protocol_error(request.id, -32600, "invalid request"));
        }

        if request.id.is_none() {
            // Notifications, including `notifications/initialized`, do not
            // receive a response. They also cannot invoke a tool.
            return None;
        }

        let id = request.id;
        let is_tool_call = request.method == "tools/call";
        let result = match request.method.as_str() {
            "initialize" => Ok(json!(InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                capabilities: Map::from_iter([("tools".to_string(), json!({}))]),
                server_info: ServerInfo {
                    name: "struxio",
                    version: env!("CARGO_PKG_VERSION"),
                },
            })),
            "tools/list" => Ok(json!(ToolsList {
                tools: tool_definitions(),
            })),
            "tools/call" => self.call_tool(request.params).await,
            "ping" => Ok(json!({})),
            _ => Err(McpError::Request {
                code: "method_not_found",
                message: "method not found".to_string(),
            }),
        };

        Some(if is_tool_call {
            match result {
                Ok(result) => self.success(id, tool_success_result(result)),
                Err(error) => self.success(id, tool_error_result(&error)),
            }
        } else {
            match result {
                Ok(result) => self.success(id, result),
                Err(_) => self.protocol_error(id, -32601, "method not found"),
            }
        })
    }

    async fn call_tool(&self, params: Option<Value>) -> McpResult<Value> {
        let params = params.ok_or_else(|| McpError::invalid_arguments("tool name is required"))?;
        let object = params
            .as_object()
            .ok_or_else(|| McpError::invalid_arguments("tools/call params must be an object"))?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::invalid_arguments("tool name is required"))?;
        let arguments = object.get("arguments").cloned();

        match name {
            "list_templates" => {
                let _: Map<String, Value> = parse_input(arguments)?;
                self.backend.list_templates().await.and_then(to_json_value)
            }
            "extract_inline" => {
                let input: InlineInput = parse_input(arguments)?;
                self.backend
                    .extract_inline(input)
                    .await
                    .and_then(to_json_value)
            }
            "get_extraction_status" => {
                let input: ExtractionStatusInput = parse_input(arguments)?;
                self.backend
                    .extraction_status(input)
                    .await
                    .and_then(to_json_value)
            }
            "submit_batch" => {
                let input: BatchInput = parse_input(arguments)?;
                self.backend
                    .submit_batch(input)
                    .await
                    .and_then(to_json_value)
            }
            "get_batch_status" => {
                let input: BatchStatusInput = parse_input(arguments)?;
                self.backend
                    .batch_status(input)
                    .await
                    .and_then(to_json_value)
            }
            _ => Err(McpError::Request {
                code: "tool_not_found",
                message: "tool not found".to_string(),
            }),
        }
    }

    fn success(&self, id: Option<Value>, result: Value) -> Vec<u8> {
        serialize_response(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        })
    }

    fn protocol_error(&self, id: Option<Value>, code: i32, message: &'static str) -> Vec<u8> {
        let data = if code == -32601 {
            Some(json!(ErrorEnvelope {
                error: ErrorDetail {
                    code: "method_not_found",
                    message,
                },
            }))
        } else {
            None
        };
        serialize_response(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
        })
    }

    /// Serve newline-delimited JSON-RPC over stdin/stdout.
    pub async fn serve_stdio(&self) -> io::Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let mut reader = tokio::io::BufReader::new(stdin);
        let mut writer = tokio::io::BufWriter::new(stdout);

        loop {
            match read_bounded_message(&mut reader, self.max_message_bytes).await? {
                ReadMessage::Eof => return Ok(()),
                ReadMessage::TooLarge => {
                    writer
                        .write_all(&self.protocol_error(None, -32600, "message too large"))
                        .await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                }
                ReadMessage::Line(line) => {
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    if let Some(response) = self.handle_json(&line).await {
                        writer.write_all(&response).await?;
                        writer.write_all(b"\n").await?;
                        writer.flush().await?;
                    }
                }
            }
        }
    }
}

fn serialize_response(response: JsonRpcResponse) -> Vec<u8> {
    serde_json::to_vec(&response).expect("JSON-RPC response types are serializable")
}

fn to_json_value<T: Serialize>(value: T) -> McpResult<Value> {
    serde_json::to_value(value).map_err(|_| McpError::Request {
        code: "serialization_error",
        message: "failed to serialize tool result".to_string(),
    })
}

fn tool_error_result(error: &McpError) -> Value {
    let envelope = ErrorEnvelope {
        error: ErrorDetail {
            code: error.code(),
            message: error.message(),
        },
    };
    json!(ToolCallResult {
        content: [ToolContent {
            content_type: "text",
            text: serde_json::to_string(&envelope).expect("error envelope is serializable"),
        }],
        is_error: true,
    })
}

fn tool_success_result(value: Value) -> Value {
    json!(ToolCallResult {
        content: [ToolContent {
            content_type: "text",
            text: serde_json::to_string(&value).expect("tool result is JSON"),
        }],
        is_error: false,
    })
}

fn tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_templates",
            description: "List extraction templates visible in the authenticated workspace.",
            input_schema: object_schema(Map::new(), Vec::new()),
        },
        Tool {
            name: "extract_inline",
            description: "Extract one base64-encoded document with a workspace template. Maximum decoded input is 8 MiB.",
            input_schema: object_schema(
                Map::from_iter([
                    ("file_name".to_string(), json!({"type": "string", "maxLength": 255})),
                    ("file_type".to_string(), json!({"type": "string", "maxLength": 64})),
                    ("file_base64".to_string(), json!({"type": "string"})),
                    (
                        "template_id".to_string(),
                        json!({"type": "string", "format": "uuid"}),
                    ),
                ]),
                vec![
                    "file_name".to_string(),
                    "file_type".to_string(),
                    "file_base64".to_string(),
                    "template_id".to_string(),
                ],
            ),
        },
        Tool {
            name: "get_extraction_status",
            description: "Get one extraction and its current result or failure status.",
            input_schema: uuid_schema("extraction_id"),
        },
        Tool {
            name: "submit_batch",
            description: "Submit up to 100 existing workspace documents for asynchronous extraction.",
            input_schema: object_schema(
                Map::from_iter([
                    (
                        "document_ids".to_string(),
                        json!({
                            "type": "array",
                            "minItems": 1,
                            "maxItems": 100,
                            "items": {"type": "string", "format": "uuid"}
                        }),
                    ),
                    (
                        "template_id".to_string(),
                        json!({"type": "string", "format": "uuid"}),
                    ),
                ]),
                vec!["document_ids".to_string(), "template_id".to_string()],
            ),
        },
        Tool {
            name: "get_batch_status",
            description: "Get one batch job and its progress in the authenticated workspace.",
            input_schema: uuid_schema("batch_id"),
        },
    ]
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

async fn read_bounded_message<R>(reader: &mut R, max_bytes: usize) -> io::Result<ReadMessage>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    let mut too_large = false;

    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            if line.is_empty() && !too_large {
                return Ok(ReadMessage::Eof);
            }
            return Ok(if too_large {
                ReadMessage::TooLarge
            } else {
                ReadMessage::Line(line)
            });
        }

        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let consume = newline.map_or(chunk.len(), |position| position + 1);
        let data_len = newline.unwrap_or(chunk.len());
        if !too_large {
            if line.len() + consume > max_bytes {
                too_large = true;
            } else {
                line.extend_from_slice(&chunk[..data_len]);
            }
        }
        reader.consume(consume);

        if newline.is_some() {
            return Ok(if too_large {
                ReadMessage::TooLarge
            } else {
                ReadMessage::Line(line)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{
        BatchInput, BatchStatusInput, ExtractionStatusInput, InlineInput, McpBackend, McpResult,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use uuid::Uuid;

    #[derive(Default)]
    struct FakeBackend;

    #[async_trait]
    impl McpBackend for FakeBackend {
        async fn list_templates(
            &self,
        ) -> McpResult<Vec<struxio_common::models::ExtractionTemplate>> {
            Ok(Vec::new())
        }

        async fn extract_inline(
            &self,
            _input: InlineInput,
        ) -> McpResult<struxio_common::models::Extraction> {
            Err(McpError::Request {
                code: "input_too_large",
                message: "decoded inline input exceeds 8388608 bytes".to_string(),
            })
        }

        async fn extraction_status(
            &self,
            _input: ExtractionStatusInput,
        ) -> McpResult<struxio_common::models::Extraction> {
            Err(McpError::from_app(struxio_common::AppError::NotFound(
                "hidden".to_string(),
            )))
        }

        async fn submit_batch(
            &self,
            _input: BatchInput,
        ) -> McpResult<struxio_common::models::BatchJob> {
            Err(McpError::invalid_arguments("not used"))
        }

        async fn batch_status(
            &self,
            _input: BatchStatusInput,
        ) -> McpResult<struxio_common::models::BatchJob> {
            Err(McpError::invalid_arguments("not used"))
        }
    }

    #[tokio::test]
    async fn tools_list_exposes_only_the_small_supported_surface() {
        let server = McpServer::new(FakeBackend);
        let response = server
            .handle_json(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        let names: Vec<&str> = value["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "list_templates",
                "extract_inline",
                "get_extraction_status",
                "submit_batch",
                "get_batch_status"
            ]
        );
    }

    #[tokio::test]
    async fn tool_errors_are_stable_json_envelopes() {
        let server = McpServer::new(FakeBackend);
        let response = server
            .handle_json(
                json!({
                    "jsonrpc": "2.0",
                    "id": "x",
                    "method": "tools/call",
                    "params": {
                        "name": "get_extraction_status",
                        "arguments": {"extraction_id": Uuid::new_v4()}
                    }
                })
                .to_string()
                .as_bytes(),
            )
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["result"]["isError"], true);
        assert_eq!(
            serde_json::from_str::<Value>(value["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap(),
            json!({"error": {"code": "not_found", "message": "resource not found"}})
        );
    }

    #[tokio::test]
    async fn malformed_protocol_messages_return_json_rpc_errors() {
        let server = McpServer::new(FakeBackend);
        let response = server.handle_json(b"{").await.unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["error"]["code"], -32700);
        assert_eq!(value["id"], Value::Null);
    }

    #[tokio::test]
    async fn notifications_do_not_write_responses() {
        let server = McpServer::new(FakeBackend);
        assert!(server
            .handle_json(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn tool_arguments_reject_unknown_fields() {
        let server = McpServer::new(FakeBackend);
        let response = server
            .handle_json(
                br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_batch_status","arguments":{"batch_id":"00000000-0000-0000-0000-000000000001","unexpected":true}}}"#,
            )
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        let text = value["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(text).unwrap()["error"]["code"],
            "invalid_arguments"
        );
    }

    #[test]
    fn bounded_message_reader_does_not_accept_a_large_line() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut reader = tokio::io::BufReader::new(&b"12345\n"[..]);
            assert!(matches!(
                read_bounded_message(&mut reader, 4).await.unwrap(),
                ReadMessage::TooLarge
            ));
        });
    }

    #[test]
    fn successful_results_are_wrapped_for_mcp_clients() {
        assert_eq!(
            tool_success_result(json!({"ok": true})),
            json!({
                "content": [{"type": "text", "text": "{\"ok\":true}"}],
                "isError": false
            })
        );
    }
}
