pub mod providers;
pub mod terminal;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use providers::{claude, codex, gemini, grokbuild, hermes, openclaw, opencode, pi};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionRequest {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionOutcome {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let (r1, r2, r3, r4, r5, r6, r7, r8, r9) = std::thread::scope(|s| {
        let h1 = s.spawn(codex::scan_sessions);
        let h2 = s.spawn(claude::scan_sessions);
        let h3 = s.spawn(opencode::scan_sessions);
        let h4 = s.spawn(openclaw::scan_sessions);
        let h5 = s.spawn(gemini::scan_sessions);
        let h6 = s.spawn(hermes::scan_sessions);
        let h7 = s.spawn(grokbuild::scan_sessions);
        let h8 = s.spawn(pi::scan_sessions);
        let h9 = s.spawn(|| {
            if crate::fork_policy::app_management_allowed(
                &crate::app_config::AppType::ClaudeCometix,
            ) && cometix_session_root().is_ok()
            {
                claude::scan_cometix_sessions()
            } else {
                Vec::new()
            }
        });
        (
            h1.join().unwrap_or_default(),
            h2.join().unwrap_or_default(),
            h3.join().unwrap_or_default(),
            h4.join().unwrap_or_default(),
            h5.join().unwrap_or_default(),
            h6.join().unwrap_or_default(),
            h7.join().unwrap_or_default(),
            h8.join().unwrap_or_default(),
            h9.join().unwrap_or_default(),
        )
    });

    let mut sessions = Vec::new();
    sessions.extend(r1);
    sessions.extend(r2);
    sessions.extend(r3);
    sessions.extend(r4);
    sessions.extend(r5);
    sessions.extend(r6);
    sessions.extend(r7);
    sessions.extend(r8);
    sessions.extend(r9);

    sessions.sort_by(|a, b| {
        let a_ts = a.last_active_at.or(a.created_at).unwrap_or(0);
        let b_ts = b.last_active_at.or(b.created_at).unwrap_or(0);
        b_ts.cmp(&a_ts)
    });

    sessions
}

pub fn load_messages(provider_id: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    // SQLite sessions use a "sqlite:" prefixed source_path
    if provider_id == "opencode" && source_path.starts_with("sqlite:") {
        return opencode::load_messages_sqlite(source_path);
    }
    if provider_id == "hermes" && source_path.starts_with("sqlite:") {
        return hermes::load_messages_sqlite(source_path);
    }

    let roots = provider_roots(provider_id)?;
    load_messages_with_roots(provider_id, Path::new(source_path), &roots)
}

fn load_messages_with_roots(
    provider_id: &str,
    source_path: &Path,
    roots: &[PathBuf],
) -> Result<Vec<SessionMessage>, String> {
    let (validated_source, _) = validate_source_path_with_roots(provider_id, source_path, roots)?;

    match provider_id {
        "codex" => codex::load_messages(&validated_source),
        "claude" | "claude-cometix" => claude::load_messages(&validated_source),
        "opencode" => opencode::load_messages(&validated_source),
        "openclaw" => openclaw::load_messages(&validated_source),
        "gemini" => gemini::load_messages(&validated_source),
        "grokbuild" => grokbuild::load_messages(&validated_source),
        "hermes" => hermes::load_messages(&validated_source),
        "pi" => pi::load_messages(&validated_source),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

pub fn delete_session(
    provider_id: &str,
    session_id: &str,
    source_path: &str,
) -> Result<bool, String> {
    if let Some(app) = session_deletion_app_type(provider_id) {
        crate::fork_policy::ensure_app_management_allowed(&app)
            .map_err(|error| error.to_string())?;
    }

    // SQLite sessions bypass the file-based deletion path
    if provider_id == "opencode" && source_path.starts_with("sqlite:") {
        return opencode::delete_session_sqlite(session_id, source_path);
    }
    if provider_id == "hermes" && source_path.starts_with("sqlite:") {
        return hermes::delete_session_sqlite(session_id, source_path);
    }

    let roots = provider_roots(provider_id)?;
    delete_session_with_roots(provider_id, session_id, Path::new(source_path), &roots)
}

fn session_deletion_app_type(provider_id: &str) -> Option<crate::app_config::AppType> {
    match provider_id {
        "claude" => Some(crate::app_config::AppType::Claude),
        "claude-cometix" => Some(crate::app_config::AppType::ClaudeCometix),
        _ => None,
    }
}

pub fn delete_sessions(requests: &[DeleteSessionRequest]) -> Vec<DeleteSessionOutcome> {
    collect_delete_session_outcomes(requests, |request| {
        delete_session(
            &request.provider_id,
            &request.session_id,
            &request.source_path,
        )
    })
}

fn delete_session_with_roots(
    provider_id: &str,
    session_id: &str,
    source_path: &Path,
    roots: &[PathBuf],
) -> Result<bool, String> {
    let (validated_source, validated_root) =
        validate_source_path_with_roots(provider_id, source_path, roots)?;

    match provider_id {
        "codex" => codex::delete_session(&validated_root, &validated_source, session_id),
        "claude" | "claude-cometix" => {
            claude::delete_session(&validated_root, &validated_source, session_id)
        }
        "opencode" => opencode::delete_session(&validated_root, &validated_source, session_id),
        "openclaw" => openclaw::delete_session(&validated_root, &validated_source, session_id),
        "gemini" => gemini::delete_session(&validated_root, &validated_source, session_id),
        "grokbuild" => grokbuild::delete_session(&validated_root, &validated_source, session_id),
        "hermes" => hermes::delete_session(&validated_root, &validated_source, session_id),
        "pi" => pi::delete_session(&validated_root, &validated_source, session_id),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

fn validate_source_path_with_roots(
    provider_id: &str,
    source_path: &Path,
    roots: &[PathBuf],
) -> Result<(PathBuf, PathBuf), String> {
    let validated_source = canonicalize_existing_path(source_path, "session source")?;

    let mut saw_existing_root = false;
    for root in roots {
        if !root.exists() {
            continue;
        }

        saw_existing_root = true;
        let validated_root = canonicalize_existing_path(root, "session root")?;
        if validated_source.starts_with(&validated_root) {
            return Ok((validated_source, validated_root));
        }
    }

    if !saw_existing_root {
        return Err(format!(
            "Session root not found for provider {provider_id}: {}",
            roots
                .first()
                .map(|root| root.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
    }

    Err(format!(
        "Session source path is outside provider roots: {}",
        source_path.display()
    ))
}

fn provider_roots(provider_id: &str) -> Result<Vec<PathBuf>, String> {
    let roots = match provider_id {
        "codex" => codex::session_roots(),
        "claude" => vec![crate::config::get_claude_config_dir().join("projects")],
        "claude-cometix" => vec![cometix_session_root()?],
        "opencode" => vec![opencode::get_opencode_data_dir()],
        "openclaw" => vec![crate::openclaw_config::get_openclaw_dir().join("agents")],
        "gemini" => vec![crate::gemini_config::get_gemini_dir().join("tmp")],
        "grokbuild" => grokbuild::session_roots(),
        "hermes" => vec![crate::hermes_config::get_hermes_dir().join("sessions")],
        "pi" => pi::session_roots(),
        _ => return Err(format!("Unsupported provider: {provider_id}")),
    };

    Ok(roots)
}

fn cometix_session_root() -> Result<PathBuf, String> {
    let root = crate::config::get_claude_cometix_config_dir().join("projects");
    crate::fork_policy::ensure_cometix_managed_config_path_isolated(&root)
        .map_err(|error| error.to_string())?;
    Ok(root)
}

fn canonicalize_existing_path(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("{label} not found: {}", path.display()));
    }

    path.canonicalize()
        .map_err(|e| format!("Failed to resolve {label} {}: {e}", path.display()))
}

fn collect_delete_session_outcomes<F>(
    requests: &[DeleteSessionRequest],
    mut deleter: F,
) -> Vec<DeleteSessionOutcome>
where
    F: FnMut(&DeleteSessionRequest) -> Result<bool, String>,
{
    requests
        .iter()
        .map(|request| match deleter(request) {
            Ok(true) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: true,
                error: None,
            },
            Ok(false) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: false,
                error: Some("Session was not deleted".to_string()),
            },
            Err(error) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: false,
                error: Some(error),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct ReloadedTestHome(Option<std::ffi::OsString>);

    impl ReloadedTestHome {
        fn set(home: &Path) -> Self {
            let guard = Self(std::env::var_os("CC_SWITCH_TEST_HOME"));
            std::env::set_var("CC_SWITCH_TEST_HOME", home);
            crate::settings::reload_settings().expect("reload isolated settings");
            guard
        }
    }

    impl Drop for ReloadedTestHome {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    #[cfg(unix)]
    fn alias_directory(source: &Path, destination: &Path) -> bool {
        std::os::unix::fs::symlink(source, destination).is_ok()
    }

    #[cfg(windows)]
    fn alias_directory(source: &Path, destination: &Path) -> bool {
        if std::os::windows::fs::symlink_dir(source, destination).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(destination)
            .arg(source)
            .status()
            .is_ok_and(|status| status.success())
    }

    fn write_codex_session(path: &Path, session_id: &str) {
        std::fs::write(
            path,
            format!(
                "{{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\",\"cwd\":\"/tmp/project\"}}}}\n\
                 {{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}}}\n",
            ),
        )
        .expect("write source");
    }

    #[test]
    fn accepts_source_path_under_any_allowed_provider_root() {
        let active_root = tempdir().expect("active root");
        let archived_root = tempdir().expect("archived root");
        let source = archived_root.path().join("session.jsonl");
        write_codex_session(&source, "archived-session");

        let deleted = delete_session_with_roots(
            "codex",
            "archived-session",
            &source,
            &[
                active_root.path().to_path_buf(),
                archived_root.path().to_path_buf(),
            ],
        )
        .expect("delete archived session");

        assert!(deleted);
        assert!(!source.exists());
    }

    #[test]
    fn rejects_source_path_outside_provider_root() {
        let root = tempdir().expect("tempdir");
        let outside = tempdir().expect("tempdir");
        let source = outside.path().join("session.jsonl");
        std::fs::write(&source, "{}").expect("write source");

        let err =
            delete_session_with_roots("codex", "session-1", &source, &[root.path().to_path_buf()])
                .expect_err("expected outside-root path to be rejected");

        assert!(err.contains("outside provider roots"));
    }

    #[test]
    fn rejects_missing_source_path() {
        let root = tempdir().expect("tempdir");
        let missing = root.path().join("missing.jsonl");

        let err =
            delete_session_with_roots("codex", "session-1", &missing, &[root.path().to_path_buf()])
                .expect_err("expected missing source path to fail");

        assert!(err.contains("session source not found"));
    }

    #[test]
    fn loads_cometix_session_messages_with_the_claude_parser() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("cometix-session.jsonl");
        std::fs::write(
            &source,
            "{\"message\":{\"role\":\"user\",\"content\":\"hello from cometix\"},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .expect("write Cometix session");

        let messages =
            load_messages_with_roots("claude-cometix", &source, &[temp.path().to_path_buf()])
                .expect("load Cometix session messages");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello from cometix");
    }

    #[test]
    #[serial_test::serial]
    fn cometix_session_root_is_independent_from_official_claude() {
        let official = provider_roots("claude").expect("official Claude roots");
        let cometix = provider_roots("claude-cometix").expect("Cometix roots");

        assert_eq!(
            official,
            vec![crate::config::get_claude_config_dir().join("projects")]
        );
        assert_eq!(
            cometix,
            vec![crate::config::get_claude_cometix_config_dir().join("projects")]
        );
        assert_ne!(official, cometix);
    }

    #[test]
    #[serial_test::serial]
    fn cometix_session_root_rejects_an_inner_alias_to_official_projects() {
        let temp = tempdir().expect("tempdir");
        let _home = ReloadedTestHome::set(temp.path());
        let official_projects = temp.path().join(".claude").join("projects");
        let cometix_root = temp.path().join(".hlclaude");
        let cometix_projects = cometix_root.join("projects");
        std::fs::create_dir_all(&official_projects).expect("create official projects");
        std::fs::create_dir_all(&cometix_root).expect("create Cometix root");
        assert!(
            alias_directory(&official_projects, &cometix_projects),
            "create Cometix projects alias"
        );

        let result = provider_roots("claude-cometix");

        #[cfg(windows)]
        std::fs::remove_dir(&cometix_projects).expect("remove test junction");

        assert!(result.is_err(), "aliased Cometix projects must be rejected");
    }

    #[test]
    fn deletes_cometix_session_only_through_its_own_root() {
        let root = tempdir().expect("Cometix root");
        let source = root.path().join("cometix-session.jsonl");
        std::fs::write(
            &source,
            concat!(
                "{\"sessionId\":\"cometix-session\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n"
            ),
        )
        .expect("write Cometix session");

        let deleted = delete_session_with_roots(
            "claude-cometix",
            "cometix-session",
            &source,
            &[root.path().to_path_buf()],
        )
        .expect("delete Cometix session");

        assert!(deleted);
        assert!(!source.exists());
    }

    #[test]
    fn cometix_delete_rejects_an_official_claude_session_path() {
        let cometix_root = tempdir().expect("Cometix root");
        let official_root = tempdir().expect("official Claude root");
        let official_source = official_root.path().join("official-session.jsonl");
        std::fs::write(
            &official_source,
            "{\"sessionId\":\"official-session\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .expect("write official session");

        let error = delete_session_with_roots(
            "claude-cometix",
            "official-session",
            &official_source,
            &[cometix_root.path().to_path_buf()],
        )
        .expect_err("Cometix must reject official Claude history paths");

        assert!(error.contains("outside provider roots"));
        assert!(official_source.exists());
    }

    #[test]
    fn cometix_load_rejects_an_official_claude_session_path() {
        let cometix_root = tempdir().expect("Cometix root");
        let official_root = tempdir().expect("official Claude root");
        let official_source = official_root.path().join("official-session.jsonl");
        std::fs::write(
            &official_source,
            "{\"message\":{\"role\":\"user\",\"content\":\"official secret\"}}\n",
        )
        .expect("write official session");

        let error = load_messages_with_roots(
            "claude-cometix",
            &official_source,
            &[cometix_root.path().to_path_buf()],
        )
        .expect_err("Cometix must reject official Claude history paths");

        assert!(error.contains("outside provider roots"));
    }

    #[test]
    fn official_claude_load_rejects_a_cometix_session_path() {
        let official_root = tempdir().expect("official Claude root");
        let cometix_root = tempdir().expect("Cometix root");
        let cometix_source = cometix_root.path().join("cometix-session.jsonl");
        std::fs::write(
            &cometix_source,
            "{\"message\":{\"role\":\"user\",\"content\":\"Cometix secret\"}}\n",
        )
        .expect("write Cometix session");

        let error = load_messages_with_roots(
            "claude",
            &cometix_source,
            &[official_root.path().to_path_buf()],
        )
        .expect_err("official Claude must reject Cometix history paths");

        assert!(error.contains("outside provider roots"));
    }

    #[test]
    fn private_fork_rejects_official_claude_session_deletion_at_service_boundary() {
        let error = delete_session("claude", "official-session", "missing.jsonl")
            .expect_err("official Claude history deletion must be disabled");
        assert!(error.contains("official CC Switch"));

        let outcomes = delete_sessions(&[DeleteSessionRequest {
            provider_id: "claude".to_string(),
            session_id: "official-session".to_string(),
            source_path: "missing.jsonl".to_string(),
        }]);
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].success);
        assert!(outcomes[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("official CC Switch")));
    }

    #[test]
    fn claude_session_deletion_uses_the_matching_product_policy_app() {
        assert_eq!(
            session_deletion_app_type("claude"),
            Some(crate::app_config::AppType::Claude)
        );
        assert_eq!(
            session_deletion_app_type("claude-cometix"),
            Some(crate::app_config::AppType::ClaudeCometix)
        );
        assert_eq!(session_deletion_app_type("codex"), None);
    }

    #[test]
    fn batch_delete_collects_successes_and_failures_in_order() {
        let requests = vec![
            DeleteSessionRequest {
                provider_id: "codex".to_string(),
                session_id: "s1".to_string(),
                source_path: "/tmp/s1".to_string(),
            },
            DeleteSessionRequest {
                provider_id: "claude".to_string(),
                session_id: "s2".to_string(),
                source_path: "/tmp/s2".to_string(),
            },
            DeleteSessionRequest {
                provider_id: "gemini".to_string(),
                session_id: "s3".to_string(),
                source_path: "/tmp/s3".to_string(),
            },
        ];

        let outcomes = collect_delete_session_outcomes(&requests, |request| {
            match request.session_id.as_str() {
                "s1" => Ok(true),
                "s2" => Err("boom".to_string()),
                _ => Ok(false),
            }
        });

        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].success);
        assert_eq!(outcomes[0].error, None);
        assert!(!outcomes[1].success);
        assert_eq!(outcomes[1].error.as_deref(), Some("boom"));
        assert!(!outcomes[2].success);
        assert_eq!(
            outcomes[2].error.as_deref(),
            Some("Session was not deleted")
        );
    }
}
