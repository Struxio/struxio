use struxio_common::AppError;

/// Represents an authenticated caller on a self-hosted Struxio instance.
/// OSS auth is presence-only: you are either authenticated or not.
/// There are no roles, orgs, or user identity concepts in the OSS API.
#[derive(Debug, Clone)]
pub struct AuthUser;

// Kept for potential future use but not enforced in OSS routes.
pub fn _require_role(_user: &AuthUser, _minimum_role: &str) -> Result<(), AppError> {
    Ok(())
}
