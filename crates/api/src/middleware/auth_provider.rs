use struxio_common::{AppError, PrincipalContext, Scope};

/// OSS and cloud both inject a real [`PrincipalContext`]. Presence-only auth
/// is no longer representable: a caller always has a non-nil workspace.
pub type AuthUser = PrincipalContext;

/// Require `scope` on the authenticated principal. Failures are authorization
/// errors and do not mention whether a resource exists.
pub fn require_scope(user: &PrincipalContext, scope: Scope) -> Result<(), AppError> {
    user.require_scope(scope)
}
