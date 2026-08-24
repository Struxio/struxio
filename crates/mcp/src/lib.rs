// SPDX-License-Identifier: AGPL-3.0-only

//! Thin stdio MCP adapter for Struxio.
//!
//! This crate speaks JSON-RPC 2.0 over newline-delimited stdio and forwards a
//! compact tool set to existing application services. It does not call
//! repositories, parse providers, evidence, or job internals.

mod backend;
mod dispatch;
mod error;
mod limits;
mod protocol;
mod server;
mod tools;

pub use backend::{McpBackend, ServiceAdapter};
pub use dispatch::dispatch_tool;
pub use error::{McpError, McpResult};
pub use limits::{
    MAX_BATCH_DOCUMENTS, MAX_DECODED_INLINE_BYTES, MAX_FILE_NAME_BYTES, MAX_FILE_TYPE_BYTES,
    MAX_MESSAGE_BYTES,
};
pub use protocol::{
    parse_inbound, Inbound, PREFERRED_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS,
};
pub use server::McpServer;
pub use tools::{tool_definitions, ToolName};
