// SPDX-License-Identifier: AGPL-3.0-only

/// Maximum decoded size accepted by inline `extract`.
pub const MAX_DECODED_INLINE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum number of documents accepted by one `extract_batch` call.
pub const MAX_BATCH_DOCUMENTS: usize = 100;
/// Maximum length of a single JSON-RPC message, including its terminating newline.
pub const MAX_MESSAGE_BYTES: usize = 12 * 1024 * 1024;
/// Maximum `file_name` length in bytes.
pub const MAX_FILE_NAME_BYTES: usize = 255;
/// Maximum `file_type` length in bytes.
pub const MAX_FILE_TYPE_BYTES: usize = 64;
