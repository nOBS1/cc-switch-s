#![allow(non_snake_case)]

use crate::session_manager;
use crate::{app_config::AppType, fork_policy::ensure_app_management_allowed};

const COMETIX_RESUME_PREFIX: &str = "hlclaude --resume ";

fn canonical_single_quoted_argument(value: &str) -> bool {
    if value.len() < 2 || !value.starts_with('\'') || !value.ends_with('\'') {
        return false;
    }

    let inner = &value[1..value.len() - 1];
    let decoded = inner.replace(r"'\''", "'");
    session_manager::terminal::shell_escape(&decoded) == value
}

fn command_mentions_executable(command: &str, executable: &str) -> bool {
    command.split_ascii_whitespace().any(|token| {
        let token = token
            .trim_matches(|character: char| {
                matches!(character, '(' | ')' | ';' | '|' | '&' | '\'' | '"' | '`')
            })
            .replace('\\', "/")
            .to_ascii_lowercase();
        let basename = token.rsplit('/').next().unwrap_or(&token);
        basename == executable
            || ["exe", "cmd", "bat", "ps1"]
                .iter()
                .any(|extension| basename == format!("{executable}.{extension}"))
    })
}

fn command_mentions_hlclaude_executable(command: &str) -> bool {
    command_mentions_executable(command, "hlclaude")
}

fn command_mentions_official_claude_executable(command: &str) -> bool {
    command_mentions_executable(command, "claude")
}

fn generated_cometix_resume_command(command: &str) -> Result<bool, String> {
    if let Some(argument) = command.strip_prefix(COMETIX_RESUME_PREFIX) {
        if canonical_single_quoted_argument(argument) {
            return Ok(true);
        }
        return Err("无法确认 hlclaude 会话恢复命令，已拒绝启动".to_string());
    }

    // The backend-generated Cometix resume command has one exact shape. Do not
    // let a lookalike skip the isolation check merely because it added a shell
    // prefix, extra argument, or command separator.
    if command_mentions_hlclaude_executable(command) {
        return Err("无法确认 hlclaude 会话恢复命令，已拒绝启动".to_string());
    }

    Ok(false)
}

fn ensure_session_launch_allowed_with<FO, FC>(
    provider_id: &str,
    command: &str,
    ensure_official: FO,
    ensure_cometix: FC,
) -> Result<(), String>
where
    FO: FnOnce() -> Result<(), String>,
    FC: FnOnce() -> Result<(), String>,
{
    match provider_id {
        "claude" => ensure_official(),
        "claude-cometix" => {
            if !generated_cometix_resume_command(command)? {
                return Err("无法确认 hlclaude 会话恢复命令，已拒绝启动".to_string());
            }
            ensure_cometix()
        }
        _ => {
            if command_mentions_official_claude_executable(command)
                || command_mentions_hlclaude_executable(command)
            {
                return Err("会话提供方与 Claude 恢复命令不匹配，已拒绝启动".to_string());
            }
            Ok(())
        }
    }
}

fn ensure_session_launch_allowed(provider_id: &str, command: &str) -> Result<(), String> {
    ensure_session_launch_allowed_with(
        provider_id,
        command,
        || ensure_app_management_allowed(&AppType::Claude).map_err(|error| error.to_string()),
        crate::commands::misc::ensure_cometix_runtime_preflight,
    )
}

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    let sessions = tauri::async_runtime::spawn_blocking(session_manager::scan_sessions)
        .await
        .map_err(|e| format!("Failed to scan sessions: {e}"))?;
    Ok(sessions)
}

#[tauri::command]
pub async fn get_session_messages(
    providerId: String,
    sourcePath: String,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    let provider_id = providerId.clone();
    let source_path = sourcePath.clone();
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_messages(&provider_id, &source_path)
    })
    .await
    .map_err(|e| format!("Failed to load session messages: {e}"))?
}

/// 在用户选定的终端里恢复一个会话。
///
/// # 安全边界：非 Claude 的普通 `command` 仍沿用可信 renderer 边界
///
/// 本命令接受 renderer 传来的任意字符串并最终交给 shell。多份外部审计把这一点
/// 报成"IPC 任意命令执行"，这里明确记录为**已知并接受的风险**，而不是待修缺陷。
///
/// 依据是本应用把 renderer 当作可信边界。支撑这一判断的是以下事实，全部逐条
/// 核实过（2026-07）：
///
/// 1. 全库仅一处 `dangerouslySetInnerHTML`（`ProviderIcon.tsx`），其入参是图标
///    **名字**，经 `hasIcon()` 把关后从手工维护的构建期注册表取 SVG——用户与
///    深链接都只能给名字，给不了标记内容
/// 2. 前端无 `eval` / `new Function`
/// 3. `tauri.conf.json` 的 `frontendDist` 指向打包产物，webview 不加载任何远程
///    源；界面里也没有 `<iframe>` / `<webview>`
/// 4. CSP 为 `script-src 'self'`——既不允许内联脚本，也不允许外部脚本
///
/// 因此"攻击者能调用本 IPC"这一前提，成立时已意味着他能以当前用户身份执行代码；
/// 那种情况下绕道本命令并不会让他多拿到任何东西。
///
/// # 什么会推翻这个结论
///
/// 上面四条任意一条不再成立，本命令就必须改成**只接收 session / provider 标识、
/// 由后端从会话记录重建命令**。具体触发条件：
///
/// - 渲染任何来自网络或配置文件的富文本 / HTML / SVG 内容
/// - 引入 `<iframe>`、`<webview>`，或让 webview 导航到远程 origin
/// - 放宽 CSP 的 `script-src`（例如为了加载第三方脚本或统计 SDK）
/// - 引入任何在 renderer 内执行外部代码的机制
///
/// 相比之下 `cwd` 的处理**不属于**这条豁免：它是磁盘上扫来的项目路径，正常使用
/// 就可能含 `$(...)`，与 renderer 是否可信无关，因此在
/// `session_manager::terminal::shell_escape` 里做了完整的单引号转义。
///
/// 官方 Claude 历史仅供读取；`providerId = claude` 始终走私有版的
/// `AppType::Claude` 管理禁令，其他 provider 也不能夹带 Claude 命令绕过。
///
/// Cometix 会话恢复是例外：后端只接受自身生成的
/// `hlclaude --resume '<session-id>'` 形态，并在真正启动终端前重新检查
/// `.hlclaude` 整棵配置树；无法确认形态或隔离状态时均拒绝启动。
#[tauri::command]
pub async fn launch_session_terminal(
    providerId: String,
    command: String,
    cwd: Option<String>,
    custom_config: Option<String>,
) -> Result<bool, String> {
    let provider_id = providerId.clone();
    let command = command.clone();
    let cwd = cwd.clone();
    let custom_config = custom_config.clone();

    // Read preferred terminal from global settings
    let preferred = crate::settings::get_preferred_terminal();
    // Map global setting terminal names to session terminal names
    // Global uses "iterm2", session terminal uses "iterm"
    let target = match preferred.as_deref() {
        Some("iterm2") => "iterm".to_string(),
        Some(t) => t.to_string(),
        None => "terminal".to_string(), // Default to Terminal.app on macOS
    };

    tauri::async_runtime::spawn_blocking(move || {
        ensure_session_launch_allowed(&provider_id, &command)?;
        session_manager::terminal::launch_terminal(
            &target,
            &command,
            cwd.as_deref(),
            custom_config.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Failed to launch terminal: {e}"))??;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn generated_cometix_resume_runs_the_isolation_guard() {
        let called = Cell::new(false);
        ensure_session_launch_allowed_with(
            "claude-cometix",
            "hlclaude --resume 'session-123'",
            || Err("official guard must not run".to_string()),
            || {
                called.set(true);
                Ok(())
            },
        )
        .expect("allow isolated Cometix resume");

        assert!(called.get());
    }

    #[test]
    fn generated_cometix_resume_accepts_canonical_shell_escaping() {
        let session_id = "session-'$(touch /tmp/never)'";
        let command = format!(
            "hlclaude --resume {}",
            session_manager::terminal::shell_escape(session_id)
        );

        assert!(generated_cometix_resume_command(&command).expect("classify generated command"));
    }

    #[test]
    fn malformed_hlclaude_resume_fails_closed_before_the_guard() {
        for command in [
            "hlclaude --resume session-123",
            "hlclaude --resume 'session-123' --dangerous",
            "env CLAUDE_CONFIG_DIR=~/.claude hlclaude --resume 'session-123'",
        ] {
            let called = Cell::new(false);
            assert!(ensure_session_launch_allowed_with(
                "claude-cometix",
                command,
                || Err("official guard must not run".to_string()),
                || {
                    called.set(true);
                    Ok(())
                },
            )
            .is_err());
            assert!(!called.get());
        }
    }

    #[test]
    fn official_claude_resume_is_rejected() {
        let called = Cell::new(false);
        let result = ensure_session_launch_allowed_with(
            "claude",
            "claude --resume 'session-123'",
            || {
                called.set(true);
                Err("official Claude is read-only".to_string())
            },
            || Err("Cometix guard must not run".to_string()),
        );

        assert!(result.is_err());
        assert!(called.get());
    }

    #[test]
    fn provider_mismatch_cannot_bypass_claude_resume_guards() {
        for command in [
            "claude --resume 'session-123'",
            "C:\\tools\\Claude.CMD --resume 'session-123'",
            "hlclaude --resume 'session-123'",
        ] {
            assert!(
                ensure_session_launch_allowed_with("codex", command, || Ok(()), || Ok(()),)
                    .is_err()
            );
        }
    }
}

#[tauri::command]
pub async fn delete_session(
    providerId: String,
    sessionId: String,
    sourcePath: String,
) -> Result<bool, String> {
    let provider_id = providerId.clone();
    let session_id = sessionId.clone();
    let source_path = sourcePath.clone();

    tauri::async_runtime::spawn_blocking(move || {
        session_manager::delete_session(&provider_id, &session_id, &source_path)
    })
    .await
    .map_err(|e| format!("Failed to delete session: {e}"))?
}

#[tauri::command]
pub async fn delete_sessions(
    items: Vec<session_manager::DeleteSessionRequest>,
) -> Result<Vec<session_manager::DeleteSessionOutcome>, String> {
    tauri::async_runtime::spawn_blocking(move || session_manager::delete_sessions(&items))
        .await
        .map_err(|e| format!("Failed to delete sessions: {e}"))
}
