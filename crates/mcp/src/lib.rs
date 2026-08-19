pub mod adapter;
pub mod protocol;

pub use adapter::{
    BatchInput, BatchStatusInput, ExtractionStatusInput, InlineInput, McpBackend, McpError,
    McpResult, ServiceAdapter, MAX_BATCH_DOCUMENTS, MAX_INLINE_BYTES, MAX_MESSAGE_BYTES,
};
pub use protocol::McpServer;
