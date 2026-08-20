// SPDX-License-Identifier: AGPL-3.0-only

/// Maximum length of a single JSON-RPC message, including its terminating newline.
pub const MAX_MESSAGE_BYTES: usize = 12 * 1024 * 1024;
/// Maximum `file_name` length in bytes.
pub const MAX_FILE_NAME_BYTES: usize = 255;
/// Maximum `file_type` length in bytes.
pub const MAX_FILE_TYPE_BYTES: usize = 64;
