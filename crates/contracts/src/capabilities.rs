// SPDX-License-Identifier: AGPL-3.0-only

//! Provider-neutral backend capability compatibility.
//! Provider IDs are opaque strings; this crate never names a vendor backend.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendCapability {
    StructuredJson,
    RawBytes,
    SourceEvidence,
    ParseIr,
    Vision,
    LongContext,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapabilities(BTreeSet<BackendCapability>);

impl BackendCapabilities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, capability: BackendCapability) -> Self {
        self.0.insert(capability);
        self
    }

    pub fn supports(&self, capability: BackendCapability) -> bool {
        self.0.contains(&capability)
    }

    pub fn iter(&self) -> impl Iterator<Item = BackendCapability> + '_ {
        self.0.iter().copied()
    }
}

/// Advertised backend. `provider_id` / `backend_id` are opaque allowlist keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendDescriptor {
    provider_id: String,
    backend_id: String,
    capabilities: BackendCapabilities,
}

impl BackendDescriptor {
    pub fn new(
        provider_id: impl Into<String>,
        backend_id: impl Into<String>,
        capabilities: BackendCapabilities,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            backend_id: backend_id.into(),
            capabilities,
        }
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn backend_id(&self) -> &str {
        &self.backend_id
    }

    pub fn capabilities(&self) -> &BackendCapabilities {
        &self.capabilities
    }
}

/// Contract-side compatibility constraints.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCompatibility {
    required_capabilities: BTreeSet<BackendCapability>,
    allowed_provider_ids: Option<BTreeSet<String>>,
    allowed_backend_ids: Option<BTreeSet<String>>,
}

impl BackendCompatibility {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn requiring(mut self, capability: BackendCapability) -> Self {
        self.required_capabilities.insert(capability);
        self
    }

    pub fn allowing_provider(mut self, provider_id: impl Into<String>) -> Self {
        self.allowed_provider_ids
            .get_or_insert_with(BTreeSet::new)
            .insert(provider_id.into());
        self
    }

    pub fn allowing_backend(mut self, backend_id: impl Into<String>) -> Self {
        self.allowed_backend_ids
            .get_or_insert_with(BTreeSet::new)
            .insert(backend_id.into());
        self
    }

    pub fn required_capabilities(&self) -> impl Iterator<Item = BackendCapability> + '_ {
        self.required_capabilities.iter().copied()
    }

    pub fn missing_capabilities(&self, backend: &BackendDescriptor) -> Vec<BackendCapability> {
        self.required_capabilities
            .iter()
            .copied()
            .filter(|capability| !backend.capabilities.supports(*capability))
            .collect()
    }

    pub fn is_satisfied_by(&self, backend: &BackendDescriptor) -> bool {
        if let Some(ids) = &self.allowed_provider_ids {
            if !ids.contains(&backend.provider_id) {
                return false;
            }
        }
        if let Some(ids) = &self.allowed_backend_ids {
            if !ids.contains(&backend.backend_id) {
                return false;
            }
        }
        self.missing_capabilities(backend).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_mismatch_and_allowlist() {
        let requirements = BackendCompatibility::new()
            .requiring(BackendCapability::StructuredJson)
            .requiring(BackendCapability::SourceEvidence)
            .allowing_provider("eval-provider");
        let direct = BackendDescriptor::new(
            "eval-provider",
            "direct",
            BackendCapabilities::new()
                .with(BackendCapability::StructuredJson)
                .with(BackendCapability::RawBytes),
        );
        assert_eq!(
            requirements.missing_capabilities(&direct),
            vec![BackendCapability::SourceEvidence]
        );
        assert!(!requirements.is_satisfied_by(&direct));

        let grounded = BackendDescriptor::new(
            "eval-provider",
            "parse",
            BackendCapabilities::new()
                .with(BackendCapability::StructuredJson)
                .with(BackendCapability::SourceEvidence),
        );
        assert!(requirements.is_satisfied_by(&grounded));
    }
}
