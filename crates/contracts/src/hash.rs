// SPDX-License-Identifier: AGPL-3.0-only

use crate::identity::Sha256ContentHash;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const CANONICAL_HASH_VERSION: u32 = 1;

pub fn semantic_sha256(value: &Value) -> Sha256ContentHash {
    let bytes = crate::canonical::canonical_json(value)
        .expect("contract domain values are serializable");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Sha256ContentHash::from_bytes(hasher.finalize().into())
}
