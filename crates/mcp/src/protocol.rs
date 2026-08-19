// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::{json, Map, Value};

use crate::error::{McpError, McpResult};

/// Preferred MCP protocol revision advertised by this server.
pub const PREFERRED_PROTOCOL_VERSION: &str = "2025-03-26";

/// Protocol revisions this stdio server will echo back from `initialize`.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// Parsed inbound JSON-RPC message.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
}

/// Parse one newline-stripped JSON-RPC message.
///
/// JSON-RPC batches (top-level arrays) are rejected. Message size must be
/// checked by the transport before calling this function.
pub fn parse_inbound(bytes: &[u8]) -> Result<Inbound, ProtocolError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ProtocolError::Parse)?;
    if value.is_array() {
        return Err(ProtocolError::BatchesNotSupported);
    }
    let object = value.as_object().ok_or(ProtocolError::InvalidRequest)?;
    match object.get("jsonrpc").and_then(Value::as_str) {
        Some("2.0") => {}
        _ => return Err(ProtocolError::InvalidRequest),
    }
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| !method.is_empty())
        .ok_or(ProtocolError::InvalidRequest)?
        .to_string();
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
    match object.get("id") {
        None => Ok(Inbound::Notification { method, params }),
        Some(id) => Ok(Inbound::Request {
            id: id.clone(),
            method,
            params,
        }),
    }
}

/// Transport / JSON-RPC failures that never invoke a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    Parse,
    InvalidRequest,
    BatchesNotSupported,
    MessageTooLarge,
    NotInitialized,
    MethodNotFound,
}

impl ProtocolError {
    pub fn code(self) -> i32 {
        match self {
            Self::Parse => -32700,
            Self::InvalidRequest
            | Self::BatchesNotSupported
            | Self::MessageTooLarge
            | Self::NotInitialized => -32600,
            Self::MethodNotFound => -32601,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Parse => "parse error",
            Self::InvalidRequest => "invalid request",
            Self::BatchesNotSupported => "json-rpc batches are not supported",
            Self::MessageTooLarge => "message too large",
            Self::NotInitialized => "server not initialized",
            Self::MethodNotFound => "method not found",
        }
    }
}

/// JSON-RPC success or error object, serialized without embedded newlines.
#[derive(Debug, Clone)]
pub struct JsonRpcResponse {
    pub id: Value,
    pub payload: JsonRpcPayload,
}

#[derive(Debug, Clone)]
pub enum JsonRpcPayload {
    Result(Value),
    Error {
        code: i32,
        message: String,
        data: Option<Value>,
    },
}

impl JsonRpcResponse {
    pub fn result(id: Value, result: Value) -> Self {
        Self {
            id,
            payload: JsonRpcPayload::Result(result),
        }
    }

    pub fn protocol_error(id: Value, error: ProtocolError) -> Self {
        Self {
            id,
            payload: JsonRpcPayload::Error {
                code: error.code(),
                message: error.message().to_string(),
                data: Some(json!({
                    "error": {
                        "code": match error {
                            ProtocolError::Parse => "parse_error",
                            ProtocolError::InvalidRequest => "invalid_request",
                            ProtocolError::BatchesNotSupported => "batches_not_supported",
                            ProtocolError::MessageTooLarge => "message_too_large",
                            ProtocolError::NotInitialized => "not_initialized",
                            ProtocolError::MethodNotFound => "method_not_found",
                        },
                        "message": error.message(),
                    }
                })),
            },
        }
    }

    pub fn invalid_params(id: Value, error: &McpError) -> Self {
        Self {
            id,
            payload: JsonRpcPayload::Error {
                code: -32602,
                message: "invalid params".to_string(),
                data: Some(
                    serde_json::to_value(error.clone().into_envelope())
                        .unwrap_or_else(|_| json!({"error":{"code":"invalid_arguments","message":"invalid params"}})),
                ),
            },
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let value = match &self.payload {
            JsonRpcPayload::Result(result) => json!({
                "jsonrpc": "2.0",
                "id": self.id,
                "result": result,
            }),
            JsonRpcPayload::Error {
                code,
                message,
                data,
            } => {
                let mut error = Map::from_iter([
                    ("code".to_string(), json!(code)),
                    ("message".to_string(), json!(message)),
                ]);
                if let Some(data) = data {
                    error.insert("data".to_string(), data.clone());
                }
                json!({
                    "jsonrpc": "2.0",
                    "id": self.id,
                    "error": error,
                })
            }
        };
        match serde_json::to_vec(&value) {
            Ok(bytes) if !bytes.contains(&b'\n') => bytes,
            Ok(_) | Err(_) => {
                br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"internal error"}}"#
                    .to_vec()
            }
        }
    }
}

/// Negotiate an MCP protocol version. Unknown client versions fall back to
/// [`PREFERRED_PROTOCOL_VERSION`].
pub fn negotiate_version(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|requested| {
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .copied()
                .find(|supported| *supported == requested)
        })
        .unwrap_or(PREFERRED_PROTOCOL_VERSION)
}

pub fn tool_success(value: Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": value.to_string(),
        }],
        "structuredContent": value,
        "isError": false
    })
}

pub fn tool_error(error: McpError) -> Value {
    let envelope = error.into_envelope();
    let text = serde_json::to_string(&envelope).unwrap_or_else(|_| {
        r#"{"error":{"code":"internal_error","message":"internal error"}}"#.to_string()
    });
    json!({
        "content": [{
            "type": "text",
            "text": text,
        }],
        "structuredContent": envelope,
        "isError": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_requests_and_notifications() {
        let request = parse_inbound(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap();
        match request {
            Inbound::Request { id, method, params } => {
                assert_eq!(id, json!(1));
                assert_eq!(method, "ping");
                assert_eq!(params, json!({}));
            }
            Inbound::Notification { .. } => panic!("expected request"),
        }

        let notification =
            parse_inbound(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        assert!(matches!(
            notification,
            Inbound::Notification { method, .. } if method == "notifications/initialized"
        ));
    }

    #[test]
    fn parser_rejects_malformed_and_non_2_0_messages() {
        assert_eq!(parse_inbound(b"{").unwrap_err(), ProtocolError::Parse);
        assert_eq!(
            parse_inbound(br#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#).unwrap_err(),
            ProtocolError::InvalidRequest
        );
        assert_eq!(
            parse_inbound(br#"{"jsonrpc":"2.0","id":1}"#).unwrap_err(),
            ProtocolError::InvalidRequest
        );
        assert_eq!(
            parse_inbound(br#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#).unwrap_err(),
            ProtocolError::BatchesNotSupported
        );
        assert_eq!(
            parse_inbound(br#""ping""#).unwrap_err(),
            ProtocolError::InvalidRequest
        );
    }

    #[test]
    fn parser_preserves_string_and_null_ids() {
        let inbound =
            parse_inbound(br#"{"jsonrpc":"2.0","id":"abc","method":"ping","params":{}}"#).unwrap();
        match inbound {
            Inbound::Request { id, .. } => assert_eq!(id, json!("abc")),
            Inbound::Notification { .. } => panic!("expected request"),
        }
        let inbound = parse_inbound(br#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#).unwrap();
        match inbound {
            Inbound::Request { id, .. } => assert_eq!(id, Value::Null),
            Inbound::Notification { .. } => panic!("expected request"),
        }
    }

    #[test]
    fn version_negotiation_falls_back_to_preferred() {
        assert_eq!(negotiate_version(Some("2024-11-05")), "2024-11-05");
        assert_eq!(negotiate_version(Some("2025-06-18")), "2025-06-18");
        assert_eq!(
            negotiate_version(Some("1999-01-01")),
            PREFERRED_PROTOCOL_VERSION
        );
        assert_eq!(negotiate_version(None), PREFERRED_PROTOCOL_VERSION);
    }

    #[test]
    fn encoded_responses_never_contain_newlines() {
        let response = JsonRpcResponse::result(json!(1), json!({"ok": true, "note": "line"}));
        let encoded = response.encode();
        assert!(!encoded.contains(&b'\n'));
        let parsed: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["result"]["ok"], true);
    }

    #[test]
    fn protocol_errors_use_stable_data_envelope() {
        let encoded = JsonRpcResponse::protocol_error(Value::Null, ProtocolError::Parse).encode();
        let parsed: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(parsed["error"]["code"], -32700);
        assert_eq!(parsed["error"]["data"]["error"]["code"], "parse_error");
    }
}
