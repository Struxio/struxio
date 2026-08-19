// SPDX-License-Identifier: AGPL-3.0-only

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};
use struxio_common::PrincipalContext;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::backend::McpBackend;
use crate::dispatch::dispatch_tool;
use crate::error::McpError;
use crate::limits::MAX_MESSAGE_BYTES;
use crate::protocol::{
    negotiate_version, parse_inbound, tool_error, tool_success, Inbound, JsonRpcResponse,
    ProtocolError,
};
use crate::tools::tool_definitions;

/// MCP JSON-RPC server. Stdout is reserved for protocol messages; logs go to stderr.
pub struct McpServer<B> {
    backend: B,
    principal: PrincipalContext,
    max_message_bytes: usize,
    initialized: AtomicBool,
}

impl<B> McpServer<B> {
    pub fn new(backend: B, principal: PrincipalContext) -> Self {
        Self {
            backend,
            principal,
            max_message_bytes: MAX_MESSAGE_BYTES,
            initialized: AtomicBool::new(false),
        }
    }

    pub fn with_max_message_bytes(mut self, max_message_bytes: usize) -> Self {
        self.max_message_bytes = max_message_bytes.max(1);
        self
    }

    pub fn principal(&self) -> &PrincipalContext {
        &self.principal
    }
}

impl<B: McpBackend> McpServer<B> {
    /// Handle one already-framed JSON-RPC message. Notifications return `None`.
    pub async fn handle_json(&self, input: &[u8]) -> Option<Vec<u8>> {
        let inbound = match parse_inbound(input) {
            Ok(inbound) => inbound,
            Err(error) => {
                return Some(JsonRpcResponse::protocol_error(Value::Null, error).encode())
            }
        };

        match inbound {
            Inbound::Notification { method, .. } => {
                if method == "notifications/initialized" {
                    // Lifecycle ack from the client. Tools are already allowed
                    // after a successful `initialize` response.
                    let _ = method;
                }
                None
            }
            Inbound::Request { id, method, params } => {
                Some(self.handle_request(id, &method, params).await)
            }
        }
    }

    async fn handle_request(&self, id: Value, method: &str, params: Value) -> Vec<u8> {
        match method {
            "initialize" => {
                let requested = params
                    .as_object()
                    .and_then(|object| object.get("protocolVersion"))
                    .and_then(Value::as_str);
                let version = negotiate_version(requested);
                self.initialized.store(true, Ordering::SeqCst);
                JsonRpcResponse::result(id, initialize_result(version)).encode()
            }
            "ping" => JsonRpcResponse::result(id, json!({})).encode(),
            "tools/list" | "tools/call" => {
                if !self.initialized.load(Ordering::SeqCst) {
                    return JsonRpcResponse::protocol_error(id, ProtocolError::NotInitialized)
                        .encode();
                }
                if method == "tools/list" {
                    JsonRpcResponse::result(id, json!({ "tools": tool_definitions() })).encode()
                } else {
                    self.call_tool(id, params).await
                }
            }
            _ => JsonRpcResponse::protocol_error(id, ProtocolError::MethodNotFound).encode(),
        }
    }

    async fn call_tool(&self, id: Value, params: Value) -> Vec<u8> {
        let Some(object) = params.as_object() else {
            return JsonRpcResponse::invalid_params(
                id,
                &McpError::invalid_arguments("tools/call params must be an object"),
            )
            .encode();
        };
        let Some(name) = object.get("name").and_then(Value::as_str) else {
            return JsonRpcResponse::invalid_params(
                id,
                &McpError::invalid_arguments("tool name is required"),
            )
            .encode();
        };
        let arguments = object
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let outcome = dispatch_tool(&self.backend, &self.principal, name, arguments).await;
        match outcome {
            Ok(value) => JsonRpcResponse::result(id, tool_success(value)).encode(),
            Err(error) => JsonRpcResponse::result(id, tool_error(error)).encode(),
        }
    }

    /// Serve newline-delimited JSON-RPC over stdin/stdout.
    pub async fn serve_stdio(&self) -> io::Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let mut reader = tokio::io::BufReader::new(stdin);
        let mut writer = tokio::io::BufWriter::new(stdout);
        self.serve(&mut reader, &mut writer).await
    }

    pub async fn serve<R, W>(&self, reader: &mut R, writer: &mut W) -> io::Result<()>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        loop {
            match read_bounded_message(reader, self.max_message_bytes).await? {
                ReadMessage::Eof => return Ok(()),
                ReadMessage::TooLarge => {
                    writer
                        .write_all(
                            &JsonRpcResponse::protocol_error(
                                Value::Null,
                                ProtocolError::MessageTooLarge,
                            )
                            .encode(),
                        )
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

#[derive(Debug)]
enum ReadMessage {
    Eof,
    Line(Vec<u8>),
    TooLarge,
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

fn initialize_result(protocol_version: &str) -> Value {
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "struxio",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Struxio extracts structured JSON from documents using named templates. Call list_templates, then extract with template_id plus file_base64 or document_id. Poll get_extraction or get_batch for status. parse, upload, evidence, providers, and generic jobs are not exposed."
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::McpResult;
    use chrono::Utc;
    use struxio_common::models::{
        BatchJob, CreateBatchRequest, CreateExtractionRequest, Extraction, ExtractionTemplate,
        InlineExtractionRequest,
    };
    use struxio_common::{PrincipalContext, PrincipalId, Scope, ScopeSet, WorkspaceId};
    use uuid::Uuid;

    struct EmptyBackend;

    impl McpBackend for EmptyBackend {
        async fn list_templates(
            &self,
            _ctx: &PrincipalContext,
        ) -> McpResult<Vec<ExtractionTemplate>> {
            Ok(Vec::new())
        }

        async fn get_template(
            &self,
            _ctx: &PrincipalContext,
            _template_id: Uuid,
        ) -> McpResult<ExtractionTemplate> {
            Err(McpError::from_app(struxio_common::AppError::NotFound(
                "hidden".into(),
            )))
        }

        async fn create_inline(
            &self,
            _ctx: &PrincipalContext,
            _request: InlineExtractionRequest,
        ) -> McpResult<Extraction> {
            Err(McpError::invalid_arguments("unused"))
        }

        async fn create_extraction(
            &self,
            _ctx: &PrincipalContext,
            _request: CreateExtractionRequest,
        ) -> McpResult<Extraction> {
            Err(McpError::invalid_arguments("unused"))
        }

        async fn get_extraction(
            &self,
            ctx: &PrincipalContext,
            extraction_id: Uuid,
        ) -> McpResult<Extraction> {
            Ok(Extraction {
                id: extraction_id,
                workspace_id: ctx.workspace_id(),
                document_id: Uuid::from_u128(1),
                template_id: Uuid::from_u128(2),
                batch_job_id: None,
                status: "completed".into(),
                result: Some(json!({"ok": true})),
                error_message: None,
                model_id: None,
                credits_charged: 0,
                input_tokens: 0,
                output_tokens: 0,
                processing_time_ms: None,
                created_at: Utc::now(),
                completed_at: None,
            })
        }

        async fn create_batch(
            &self,
            _ctx: &PrincipalContext,
            _request: CreateBatchRequest,
        ) -> McpResult<BatchJob> {
            Err(McpError::invalid_arguments("unused"))
        }

        async fn get_batch(&self, _ctx: &PrincipalContext, _batch_id: Uuid) -> McpResult<BatchJob> {
            Err(McpError::invalid_arguments("unused"))
        }
    }

    fn server() -> McpServer<EmptyBackend> {
        McpServer::new(
            EmptyBackend,
            PrincipalContext::new(
                WorkspaceId::local(),
                PrincipalId::local(),
                ScopeSet::from_scopes(Scope::ALL),
            ),
        )
    }

    async fn ready() -> McpServer<EmptyBackend> {
        let server = server();
        let _ = server
            .handle_json(
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
            )
            .await;
        server
    }

    #[tokio::test]
    async fn initialize_negotiates_supported_version() {
        let server = server();
        let response = server
            .handle_json(
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
            )
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(value["result"]["capabilities"]["tools"], json!({}));
        assert!(value["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("list_templates"));
    }

    #[tokio::test]
    async fn tools_require_initialize() {
        let server = server();
        let response = server
            .handle_json(br#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["error"]["code"], -32600);
        assert_eq!(value["error"]["data"]["error"]["code"], "not_initialized");
    }

    #[tokio::test]
    async fn tools_list_matches_catalog() {
        let server = ready().await;
        let response = server
            .handle_json(br#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
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
            [
                "list_templates",
                "get_template",
                "extract",
                "get_extraction",
                "extract_batch",
                "get_batch"
            ]
        );
    }

    #[tokio::test]
    async fn tool_errors_are_iserror_envelopes_not_json_rpc_errors() {
        let server = ready().await;
        let response = server
            .handle_json(
                br#"{"jsonrpc":"2.0","id":"x","method":"tools/call","params":{"name":"get_template","arguments":{"template_id":"00000000-0000-0000-0000-000000000001"}}}"#,
            )
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert!(value.get("error").is_none());
        assert_eq!(value["result"]["isError"], true);
        let body: Value =
            serde_json::from_str(value["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["message"], "resource not found");
        assert_eq!(
            value["result"]["structuredContent"]["error"]["code"],
            "not_found"
        );
    }

    #[tokio::test]
    async fn notifications_and_unknown_methods_follow_json_rpc() {
        let server = ready().await;
        assert!(server
            .handle_json(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await
            .is_none());
        let response = server
            .handle_json(br#"{"jsonrpc":"2.0","id":9,"method":"resources/list"}"#)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn ping_works_before_initialize() {
        let server = server();
        let response = server
            .handle_json(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["result"], json!({}));
    }

    #[test]
    fn bounded_reader_rejects_oversized_lines_and_resyncs() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut reader = tokio::io::BufReader::new(&b"12345\nnext\n"[..]);
            assert!(matches!(
                read_bounded_message(&mut reader, 4).await.unwrap(),
                ReadMessage::TooLarge
            ));
            match read_bounded_message(&mut reader, 8).await.unwrap() {
                ReadMessage::Line(line) => assert_eq!(line, b"next"),
                other => panic!("expected resynced line, got {other:?}"),
            }
        });
    }

    #[test]
    fn preferred_protocol_version_is_advertised_as_fallback() {
        assert_eq!(PREFERRED_PROTOCOL_VERSION, "2025-03-26");
    }
}
