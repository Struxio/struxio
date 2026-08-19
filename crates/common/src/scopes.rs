// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::BTreeSet;
use std::fmt;

use crate::AppError;

/// `documents:read` — list or fetch documents in the caller's workspace.
pub const DOCUMENTS_READ: &str = "documents:read";
/// `documents:write` — upload, confirm, or delete documents.
pub const DOCUMENTS_WRITE: &str = "documents:write";
/// `templates:read` — list or fetch extraction templates.
pub const TEMPLATES_READ: &str = "templates:read";
/// `templates:write` — create, update, or delete templates.
pub const TEMPLATES_WRITE: &str = "templates:write";
/// `extractions:create` — start a synchronous or inline extraction.
pub const EXTRACTIONS_CREATE: &str = "extractions:create";
/// `extractions:read` — list or fetch extractions.
pub const EXTRACTIONS_READ: &str = "extractions:read";
/// `batches:create` — enqueue a batch extraction job.
pub const BATCHES_CREATE: &str = "batches:create";
/// `batches:read` — list or fetch batches and their child extractions.
pub const BATCHES_READ: &str = "batches:read";

/// Typed authorization scope. Unknown strings are not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    DocumentsRead,
    DocumentsWrite,
    TemplatesRead,
    TemplatesWrite,
    ExtractionsCreate,
    ExtractionsRead,
    BatchesCreate,
    BatchesRead,
}

impl Scope {
    /// All scopes granted to the local OSS operator principal.
    pub const ALL: [Scope; 8] = [
        Scope::DocumentsRead,
        Scope::DocumentsWrite,
        Scope::TemplatesRead,
        Scope::TemplatesWrite,
        Scope::ExtractionsCreate,
        Scope::ExtractionsRead,
        Scope::BatchesCreate,
        Scope::BatchesRead,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Scope::DocumentsRead => DOCUMENTS_READ,
            Scope::DocumentsWrite => DOCUMENTS_WRITE,
            Scope::TemplatesRead => TEMPLATES_READ,
            Scope::TemplatesWrite => TEMPLATES_WRITE,
            Scope::ExtractionsCreate => EXTRACTIONS_CREATE,
            Scope::ExtractionsRead => EXTRACTIONS_READ,
            Scope::BatchesCreate => BATCHES_CREATE,
            Scope::BatchesRead => BATCHES_READ,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            DOCUMENTS_READ => Some(Scope::DocumentsRead),
            DOCUMENTS_WRITE => Some(Scope::DocumentsWrite),
            TEMPLATES_READ => Some(Scope::TemplatesRead),
            TEMPLATES_WRITE => Some(Scope::TemplatesWrite),
            EXTRACTIONS_CREATE => Some(Scope::ExtractionsCreate),
            EXTRACTIONS_READ => Some(Scope::ExtractionsRead),
            BATCHES_CREATE => Some(Scope::BatchesCreate),
            BATCHES_READ => Some(Scope::BatchesRead),
            _ => None,
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Ordered set of scopes attached to a principal in a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScopeSet {
    inner: BTreeSet<Scope>,
}

impl ScopeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn all() -> Self {
        Self {
            inner: Scope::ALL.into_iter().collect(),
        }
    }

    pub fn from_scopes<I>(scopes: I) -> Self
    where
        I: IntoIterator<Item = Scope>,
    {
        Self {
            inner: scopes.into_iter().collect(),
        }
    }

    /// Parse stored scope strings. Unknown values are ignored so cloud can
    /// introduce scopes the OSS binary does not yet understand.
    pub fn parse<I, S>(items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::from_scopes(items.into_iter().filter_map(|s| Scope::parse(s.as_ref())))
    }

    pub fn contains(&self, scope: Scope) -> bool {
        self.inner.contains(&scope)
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = Scope> + '_ {
        self.inner.iter().copied()
    }

    pub fn require(&self, scope: Scope) -> Result<(), AppError> {
        if self.contains(scope) {
            Ok(())
        } else {
            Err(AppError::Forbidden("insufficient scope".to_string()))
        }
    }

    pub fn as_strings(&self) -> Vec<String> {
        self.inner.iter().map(|s| s.as_str().to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trips_known_scopes() {
        for scope in Scope::ALL {
            assert_eq!(Scope::parse(scope.as_str()), Some(scope));
        }
    }

    #[test]
    fn parse_ignores_unknown_scopes() {
        let set = ScopeSet::parse(["documents:read", "billing:admin"]);
        assert!(set.contains(Scope::DocumentsRead));
        assert_eq!(set.iter().count(), 1);
    }

    #[test]
    fn require_denies_missing_scope_without_resource_detail() {
        let set = ScopeSet::from_scopes([Scope::DocumentsRead]);
        let err = set.require(Scope::DocumentsWrite).unwrap_err();
        match err {
            AppError::Forbidden(msg) => assert_eq!(msg, "insufficient scope"),
            other => panic!("unexpected error: {other}"),
        }
    }
}
