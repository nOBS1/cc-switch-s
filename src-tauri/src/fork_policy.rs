//! Product-level capability policy for this private fork.
//!
//! Claude Desktop is a single, machine-wide application.  This fork must never
//! manage its live configuration because doing so would make the private build
//! compete with the official CC Switch installation.  Keep the restriction at
//! the command/startup boundary so legacy database rows remain readable while
//! all externally reachable mutations are denied.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::profile::ProfileScope;

pub(crate) fn app_management_allowed(app: &AppType) -> bool {
    !matches!(app, AppType::ClaudeDesktop)
}

pub(crate) fn ensure_app_management_allowed(app: &AppType) -> Result<(), AppError> {
    if app_management_allowed(app) {
        return Ok(());
    }

    Err(AppError::localized(
        "claude_desktop_management_disabled",
        "此魔改版不管理 Claude Desktop，以免与官方 CC Switch 共用并覆盖配置",
        "Claude Desktop management is disabled in this private build to avoid conflicting with the official CC Switch installation",
    ))
}

pub(crate) fn profile_scope_management_allowed(scope: ProfileScope) -> bool {
    !matches!(scope, ProfileScope::ClaudeDesktop)
}

pub(crate) fn ensure_profile_scope_management_allowed(scope: ProfileScope) -> Result<(), AppError> {
    if profile_scope_management_allowed(scope) {
        return Ok(());
    }

    ensure_app_management_allowed(&AppType::ClaudeDesktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_fork_rejects_only_claude_desktop_app_management() {
        assert!(ensure_app_management_allowed(&AppType::ClaudeDesktop).is_err());
        assert!(ensure_app_management_allowed(&AppType::Claude).is_ok());
        assert!(ensure_app_management_allowed(&AppType::ClaudeCometix).is_ok());
        assert!(ensure_app_management_allowed(&AppType::Codex).is_ok());
    }

    #[test]
    fn private_fork_rejects_only_claude_desktop_profile_scope() {
        assert!(ensure_profile_scope_management_allowed(ProfileScope::ClaudeDesktop).is_err());
        assert!(ensure_profile_scope_management_allowed(ProfileScope::Claude).is_ok());
        assert!(ensure_profile_scope_management_allowed(ProfileScope::Codex).is_ok());
    }
}
