// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt::{self, Write as _},
    str::FromStr,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    EmptySlug,
    InvalidSlug,
    NonPositiveVersion,
    InvalidHash,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySlug => write!(f, "contract slug must not be empty"),
            Self::InvalidSlug => write!(f, "contract slug must be lowercase kebab-case"),
            Self::NonPositiveVersion => write!(f, "contract version must be positive"),
            Self::InvalidHash => write!(f, "content hash must be 64 hexadecimal characters"),
        }
    }
}

impl std::error::Error for IdentityError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContractSlug(String);

impl ContractSlug {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        if value.is_empty() {
            return Err(IdentityError::EmptySlug);
        }
        let valid = value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });
        if !valid {
            return Err(IdentityError::InvalidSlug);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ContractSlug {
    type Error = IdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ContractSlug> for String {
    fn from(value: ContractSlug) -> Self {
        value.0
    }
}

impl fmt::Display for ContractSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct PositiveVersion(u32);

impl PositiveVersion {
    pub fn new(value: u32) -> Result<Self, IdentityError> {
        (value > 0)
            .then_some(Self(value))
            .ok_or(IdentityError::NonPositiveVersion)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for PositiveVersion {
    type Error = IdentityError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PositiveVersion> for u32 {
    fn from(value: PositiveVersion) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256ContentHash([u8; 32]);

impl Sha256ContentHash {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_content(content: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(content);
        Self(hasher.finalize().into())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn as_hex(&self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
        }
        output
    }
}

impl FromStr for Sha256ContentHash {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(IdentityError::InvalidHash);
        }
        let mut bytes = [0; 32];
        for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (hex_nibble(chunk[0]) << 4) | hex_nibble(chunk[1]);
        }
        Ok(Self(bytes))
    }
}

impl TryFrom<String> for Sha256ContentHash {
    type Error = IdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Sha256ContentHash> for String {
    fn from(value: Sha256ContentHash) -> Self {
        value.as_hex()
    }
}

impl Serialize for Sha256ContentHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.as_hex())
    }
}

impl<'de> Deserialize<'de> for Sha256ContentHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Sha256ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_hex().fmt(f)
    }
}

/// Immutable identity `(slug, version, content_hash)`.
///
/// Slug and version are labels; `content_hash` is the canonical semantic SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContractIdentity {
    slug: ContractSlug,
    version: PositiveVersion,
    content_hash: Sha256ContentHash,
}

impl ContractIdentity {
    pub fn new(
        slug: ContractSlug,
        version: PositiveVersion,
        content_hash: Sha256ContentHash,
    ) -> Self {
        Self {
            slug,
            version,
            content_hash,
        }
    }

    pub fn slug(&self) -> &ContractSlug {
        &self.slug
    }

    pub fn version(&self) -> PositiveVersion {
        self.version
    }

    pub fn content_hash(&self) -> Sha256ContentHash {
        self.content_hash
    }
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => unreachable!("validated hexadecimal input"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_rejects_empty_and_uppercase() {
        assert_eq!(ContractSlug::new(""), Err(IdentityError::EmptySlug));
        assert_eq!(
            ContractSlug::new("Invoice"),
            Err(IdentityError::InvalidSlug)
        );
        assert_eq!(
            ContractSlug::new("in--voice"),
            Err(IdentityError::InvalidSlug)
        );
        assert!(ContractSlug::new("invoice-v2").is_ok());
    }

    #[test]
    fn version_must_be_positive() {
        assert_eq!(
            PositiveVersion::new(0),
            Err(IdentityError::NonPositiveVersion)
        );
        assert_eq!(PositiveVersion::new(1).unwrap().get(), 1);
    }

    #[test]
    fn content_hash_round_trips_hex() {
        let hash = Sha256ContentHash::from_content(b"payload");
        let parsed: Sha256ContentHash = hash.as_hex().parse().unwrap();
        assert_eq!(hash, parsed);
    }
}
