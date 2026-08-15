use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

use crate::app_config::{AppType, McpApps, McpServer};
use crate::error::AppError;
use crate::mcp;
use crate::store::AppState;

/// MCP 相关业务逻辑（v3.7.0 统一结构）
pub struct McpService;

/// The database schema has a single string primary key for each MCP definition.
/// Official Claude and Cometix may legitimately use the same live id with
/// different definitions, so their database identities need a private scope.
/// The scope is stripped before writing either live `.claude.json` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeMcpScope {
    Official,
    Cometix,
}

impl ClaudeMcpScope {
    const OFFICIAL_PREFIX: &'static str = "cc-switch-scope:v1:claude:";
    const COMETIX_PREFIX: &'static str = "cc-switch-scope:v1:claude-cometix:";

    fn for_app(app: &AppType) -> Option<Self> {
        match app {
            AppType::Claude => Some(Self::Official),
            AppType::ClaudeCometix => Some(Self::Cometix),
            _ => None,
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::Official => Self::OFFICIAL_PREFIX,
            Self::Cometix => Self::COMETIX_PREFIX,
        }
    }

    fn storage_id(self, live_id: &str) -> String {
        format!("{}{}", self.prefix(), URL_SAFE_NO_PAD.encode(live_id))
    }

    fn from_storage_id(storage_id: &str) -> Option<(Self, String)> {
        let (scope, encoded) = if let Some(encoded) = storage_id.strip_prefix(Self::OFFICIAL_PREFIX)
        {
            (Self::Official, encoded)
        } else if let Some(encoded) = storage_id.strip_prefix(Self::COMETIX_PREFIX) {
            (Self::Cometix, encoded)
        } else {
            return None;
        };

        let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
        let live_id = String::from_utf8(bytes).ok()?;
        Some((scope, live_id))
    }
}

fn live_mcp_id(storage_id: &str) -> String {
    ClaudeMcpScope::from_storage_id(storage_id)
        .map(|(_, live_id)| live_id)
        .unwrap_or_else(|| storage_id.to_string())
}

fn belongs_to_app(storage_id: &str, app: &AppType) -> bool {
    let Some((scope, _)) = ClaudeMcpScope::from_storage_id(storage_id) else {
        // Unscoped rows are legacy/shared rows. Cometix rows are migrated before
        // projection; treating an unscoped row as official prevents it from
        // deleting a same-id Cometix definition.
        return !matches!(app, AppType::ClaudeCometix);
    };

    match ClaudeMcpScope::for_app(app) {
        Some(app_scope) => app_scope == scope,
        None => true,
    }
}

impl McpService {
    /// 获取所有 MCP 服务器（统一结构）
    pub fn get_all_servers(state: &AppState) -> Result<IndexMap<String, McpServer>, AppError> {
        Self::migrate_legacy_cometix_rows(state)?;
        state.db.get_all_mcp_servers()
    }

    /// 添加或更新 MCP 服务器
    pub fn upsert_server(state: &AppState, server: McpServer) -> Result<(), AppError> {
        Self::migrate_legacy_cometix_rows(state)?;

        let parsed_scope = ClaudeMcpScope::from_storage_id(&server.id);
        let live_id = parsed_scope
            .as_ref()
            .map(|(_, live_id)| live_id.clone())
            .unwrap_or_else(|| server.id.clone());
        let wants_official = server.apps.claude;
        let wants_cometix = server.apps.claude_cometix;

        if parsed_scope.as_ref().map(|(scope, _)| *scope) == Some(ClaudeMcpScope::Cometix) {
            let mut cometix = server.clone();
            cometix.id = ClaudeMcpScope::Cometix.storage_id(&live_id);
            cometix.apps.claude = false;
            cometix.apps.claude_cometix = wants_cometix;
            Self::upsert_single(state, cometix)?;

            if wants_official {
                let existing = state.db.get_all_mcp_servers()?;
                let mut official = if let Some(existing) = existing.get(&live_id) {
                    existing.clone()
                } else {
                    let mut official = server;
                    official.id = live_id;
                    official.apps = McpApps::default();
                    official
                };
                official.apps.claude = true;
                official.apps.claude_cometix = false;
                Self::upsert_single(state, official)?;
            }
            return Ok(());
        }

        if wants_cometix {
            let mut shared = server.clone();
            shared.id = live_id.clone();
            shared.apps.claude_cometix = false;
            if !shared.apps.is_empty() {
                Self::upsert_single(state, shared)?;
            } else {
                // The caller edited the raw/unified row and moved its last
                // assignment to Cometix. Leaving the previous raw row behind
                // would keep official Claude enabled and its live entry stale.
                Self::delete_server(state, &live_id)?;
            }

            let cometix_id = ClaudeMcpScope::Cometix.storage_id(&live_id);
            let existing = state.db.get_all_mcp_servers()?;
            let mut cometix = if let Some(existing) = existing.get(&cometix_id) {
                existing.clone()
            } else {
                let mut cometix = server;
                cometix.id = cometix_id;
                cometix.apps = McpApps::default();
                cometix
            };
            cometix.apps.claude = false;
            cometix.apps.claude_cometix = true;
            Self::upsert_single(state, cometix)?;
            return Ok(());
        }

        if parsed_scope.as_ref().map(|(scope, _)| *scope) == Some(ClaudeMcpScope::Official) {
            let mut official = server;
            official.id = live_id;
            official.apps.claude_cometix = false;
            return Self::upsert_single(state, official);
        }

        Self::upsert_single(state, server)
    }

    fn upsert_single(state: &AppState, server: McpServer) -> Result<(), AppError> {
        // 读取旧状态：用于处理“编辑时取消勾选某个应用”的场景（需要从对应 live 配置中移除）
        let prev_apps = state
            .db
            .get_all_mcp_servers()?
            .get(&server.id)
            .map(|s| s.apps.clone())
            .unwrap_or_default();

        state.db.save_mcp_server(&server)?;

        // Reconcile once per affected target + live id. Multiple storage rows
        // may represent that same live id, so removing/syncing this row alone
        // can erase or overwrite an enabled sibling definition.
        let mut affected_apps = prev_apps.enabled_apps();
        for app in server.apps.enabled_apps() {
            if !affected_apps.contains(&app) {
                affected_apps.push(app);
            }
        }
        let live_id = live_mcp_id(&server.id);
        for app in affected_apps {
            Self::reconcile_live_id_for_app(state, &live_id, &app)?;
        }

        Ok(())
    }

    /// Move rows written by the first Cometix implementation out of the shared
    /// id namespace. Official/other flags stay on the legacy row; Cometix gets
    /// an independent copy. This is idempotent and does not touch live files.
    fn migrate_legacy_cometix_rows(state: &AppState) -> Result<(), AppError> {
        let existing = state.db.get_all_mcp_servers()?;
        for server in existing.values() {
            if ClaudeMcpScope::from_storage_id(&server.id).is_some() || !server.apps.claude_cometix
            {
                continue;
            }

            let mut cometix = server.clone();
            cometix.id = ClaudeMcpScope::Cometix.storage_id(&server.id);
            cometix.apps = McpApps {
                claude_cometix: true,
                ..McpApps::default()
            };
            if !existing.contains_key(&cometix.id) {
                state.db.save_mcp_server(&cometix)?;
            }

            let mut remainder = server.clone();
            remainder.apps.claude_cometix = false;
            if remainder.apps.is_empty() {
                state.db.delete_mcp_server(&server.id)?;
            } else {
                state.db.save_mcp_server(&remainder)?;
            }
        }
        Ok(())
    }

    /// 删除 MCP 服务器
    pub fn delete_server(state: &AppState, id: &str) -> Result<bool, AppError> {
        let server = state.db.get_all_mcp_servers()?.shift_remove(id);

        if let Some(server) = server {
            state.db.delete_mcp_server(id)?;

            // Reconcile every affected live target after deleting this storage
            // row. A same-live-id sibling may still own the target.
            Self::remove_server_from_all_apps(state, id, &server)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 切换指定应用的启用状态
    pub fn toggle_app(
        state: &AppState,
        server_id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        Self::migrate_legacy_cometix_rows(state)?;

        if let Some(scope) = ClaudeMcpScope::for_app(&app) {
            let live_id = live_mcp_id(server_id);
            let target_id = match scope {
                ClaudeMcpScope::Official => live_id.clone(),
                ClaudeMcpScope::Cometix => scope.storage_id(&live_id),
            };

            if target_id != server_id {
                // A toggle on the opposite domain's row means "make this
                // definition available in the other domain". It must never set
                // both Claude flags on one storage row, because future edits
                // would then overwrite both live configurations.
                if !enabled {
                    return Ok(());
                }

                let servers = state.db.get_all_mcp_servers()?;
                let target = if let Some(existing) = servers.get(&target_id) {
                    let mut target = existing.clone();
                    target.apps.set_enabled_for(&app, true);
                    target
                } else if let Some(source) = servers.get(server_id) {
                    let mut target = source.clone();
                    target.id = target_id;
                    target.apps = McpApps::default();
                    target.apps.set_enabled_for(&app, true);
                    target
                } else {
                    return Ok(());
                };

                state.db.save_mcp_server(&target)?;
                Self::reconcile_live_id_for_app(state, &live_id, &app)?;
                return Ok(());
            }
        }

        if let Some(server) = state
            .db
            .update_mcp_server_app_enabled(server_id, &app, enabled)?
        {
            Self::reconcile_live_id_for_app(state, &live_mcp_id(&server.id), &app)?;
        }

        Ok(())
    }

    /// 将 MCP 服务器同步到指定应用
    fn sync_server_to_app(
        _state: &AppState,
        server: &McpServer,
        app: &AppType,
    ) -> Result<(), AppError> {
        Self::sync_server_to_app_no_config(server, app)
    }

    fn sync_server_to_app_no_config(server: &McpServer, app: &AppType) -> Result<(), AppError> {
        if !belongs_to_app(&server.id, app) {
            return Ok(());
        }
        let live_id = live_mcp_id(&server.id);
        match app {
            AppType::Claude => {
                mcp::sync_single_server_to_claude(&Default::default(), &live_id, &server.server)?;
            }
            AppType::ClaudeCometix => {
                mcp::sync_single_server_to_claude_cometix(
                    &Default::default(),
                    &live_id,
                    &server.server,
                )?;
            }
            AppType::ClaudeDesktop => {
                log::debug!("Claude Desktop 3P profiles do not use CC Switch MCP sync, skipping");
            }
            AppType::Codex => {
                // Codex uses TOML format, must use the correct function
                mcp::sync_single_server_to_codex(&Default::default(), &live_id, &server.server)?;
            }
            AppType::Gemini => {
                mcp::sync_single_server_to_gemini(&Default::default(), &live_id, &server.server)?;
            }
            AppType::GrokBuild => {
                mcp::sync_single_server_to_grokbuild(
                    &Default::default(),
                    &live_id,
                    &server.server,
                )?;
            }
            AppType::OpenCode => {
                mcp::sync_single_server_to_opencode(&Default::default(), &live_id, &server.server)?;
            }
            AppType::OpenClaw => {
                // OpenClaw MCP support is still in development (Issue #4834)
                // Skip for now
                log::debug!("OpenClaw MCP support is still in development, skipping sync");
            }
            AppType::Hermes => {
                mcp::sync_single_server_to_hermes(&Default::default(), &live_id, &server.server)?;
            }
        }
        Ok(())
    }

    /// Prefer the historical unscoped/unified row for non-Claude targets. If
    /// it is disabled, a scoped definition may own that target instead. This
    /// makes the result deterministic and prevents two same-live-id rows from
    /// overwriting each other based on database iteration order.
    fn target_candidate_priority(storage_id: &str) -> u8 {
        match ClaudeMcpScope::from_storage_id(storage_id).map(|(scope, _)| scope) {
            None => 0,
            Some(ClaudeMcpScope::Official) => 1,
            Some(ClaudeMcpScope::Cometix) => 2,
        }
    }

    fn reconcile_live_id_in_servers(
        state: &AppState,
        servers: &IndexMap<String, McpServer>,
        live_id: &str,
        app: &AppType,
    ) -> Result<(), AppError> {
        let selected = Self::select_target_candidate(servers, live_id, app);

        if let Some(server) = selected {
            Self::sync_server_to_app(state, server, app)
        } else {
            Self::remove_live_id_from_app(live_id, app)
        }
    }

    fn select_target_candidate<'a>(
        servers: &'a IndexMap<String, McpServer>,
        live_id: &str,
        app: &AppType,
    ) -> Option<&'a McpServer> {
        servers
            .values()
            .filter(|server| {
                live_mcp_id(&server.id) == live_id
                    && belongs_to_app(&server.id, app)
                    && server.apps.is_enabled_for(app)
            })
            .min_by_key(|server| Self::target_candidate_priority(&server.id))
    }

    fn reconcile_live_id_for_app(
        state: &AppState,
        live_id: &str,
        app: &AppType,
    ) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;
        Self::reconcile_live_id_in_servers(state, &servers, live_id, app)
    }

    /// 从所有曾启用过该服务器的应用中移除
    fn remove_server_from_all_apps(
        state: &AppState,
        id: &str,
        server: &McpServer,
    ) -> Result<(), AppError> {
        let live_id = live_mcp_id(id);
        for app in server.apps.enabled_apps() {
            Self::reconcile_live_id_for_app(state, &live_id, &app)?;
        }
        Ok(())
    }

    fn remove_live_id_from_app(live_id: &str, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => mcp::remove_server_from_claude(live_id)?,
            AppType::ClaudeCometix => mcp::remove_server_from_claude_cometix(live_id)?,
            AppType::ClaudeDesktop => {
                log::debug!("Claude Desktop 3P profiles do not use CC Switch MCP sync, skipping");
            }
            AppType::Codex => mcp::remove_server_from_codex(live_id)?,
            AppType::Gemini => mcp::remove_server_from_gemini(live_id)?,
            AppType::GrokBuild => mcp::remove_server_from_grokbuild(live_id)?,
            AppType::OpenCode => {
                mcp::remove_server_from_opencode(live_id)?;
            }
            AppType::OpenClaw => {
                // OpenClaw MCP support is still in development
                log::debug!("OpenClaw MCP support is still in development, skipping remove");
            }
            AppType::Hermes => {
                mcp::remove_server_from_hermes(live_id)?;
            }
        }
        Ok(())
    }

    /// 手动同步所有启用的 MCP 服务器到对应的应用。
    ///
    /// Best-effort：单个应用投影失败（如 ~/.claude.json 坏 JSON）不阻断
    /// 其余应用——各应用的 live 文件互相独立，一处损坏没有理由让其他
    /// 应用的 MCP 状态陈旧。全部跑完后若有失败，聚合成一个错误上报，
    /// 保留调用方的可见性。
    pub fn sync_all_enabled(state: &AppState) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;

        let mut failures: Vec<String> = Vec::new();
        for app in AppType::all().filter(crate::fork_policy::app_management_allowed) {
            if let Err(err) = Self::project_servers_to_app(state, &servers, &app) {
                log::warn!("同步 MCP 到 {app:?} 失败: {err}");
                failures.push(format!("{}: {err}", app.as_str()));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "部分应用 MCP 同步失败: {}",
                failures.join("; ")
            )))
        }
    }

    /// 只把启用状态投影到单个应用。某个应用的 live 被整体重写后用它做
    /// 定向重投影，避免把无关应用的失败面（如 ~/.claude.json 坏 JSON）
    /// 牵连进目标应用的关键路径。
    pub fn sync_enabled_for_app(state: &AppState, app: &AppType) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;
        Self::project_servers_to_app(state, &servers, app)
    }

    fn project_servers_to_app(
        state: &AppState,
        servers: &IndexMap<String, McpServer>,
        app: &AppType,
    ) -> Result<(), AppError> {
        if matches!(app, AppType::OpenClaw | AppType::ClaudeDesktop) {
            return Ok(());
        }

        let mut reconciled = HashSet::new();
        for server in servers.values() {
            if !belongs_to_app(&server.id, app) {
                continue;
            }
            let live_id = live_mcp_id(&server.id);
            if reconciled.insert(live_id.clone()) {
                Self::reconcile_live_id_in_servers(state, servers, &live_id, app)?;
            }
        }

        Ok(())
    }

    // ========================================================================
    // 兼容层：支持旧的 v3.6.x 命令（已废弃，将在 v4.0 移除）
    // ========================================================================

    /// [已废弃] 获取指定应用的 MCP 服务器（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use get_all_servers instead")]
    pub fn get_servers(
        state: &AppState,
        app: AppType,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let all_servers = Self::get_all_servers(state)?;
        let mut result = HashMap::new();
        let mut visited = HashSet::new();

        for server in all_servers.values() {
            if !belongs_to_app(&server.id, &app) {
                continue;
            }
            let live_id = live_mcp_id(&server.id);
            if visited.insert(live_id.clone()) {
                if let Some(selected) = Self::select_target_candidate(&all_servers, &live_id, &app)
                {
                    result.insert(live_id, selected.server.clone());
                }
            }
        }

        Ok(result)
    }

    /// [已废弃] 设置 MCP 服务器在指定应用的启用状态（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use toggle_app instead")]
    pub fn set_enabled(
        state: &AppState,
        app: AppType,
        id: &str,
        enabled: bool,
    ) -> Result<bool, AppError> {
        Self::toggle_app(state, id, app, enabled)?;
        Ok(true)
    }

    /// [已废弃] 同步启用的 MCP 到指定应用（兼容旧 API）
    #[deprecated(since = "3.7.0", note = "Use sync_all_enabled instead")]
    pub fn sync_enabled(state: &AppState, app: AppType) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;
        Self::project_servers_to_app(state, &servers, &app)
    }

    /// 从 Claude 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_claude(state: &AppState) -> Result<usize, AppError> {
        Self::migrate_legacy_cometix_rows(state)?;
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_claude(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // Official Claude keeps the historical unified row so its
                    // sharing with Codex/Gemini/etc. remains unchanged. Only
                    // Cometix needs a private id namespace.
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.claude = true;
                        merged.apps.claude_cometix = false;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Claude Code (Cometix) 独立配置导入 MCP。
    /// 导入只更新数据库中的 Cometix 启用标志，不反向同步 live 配置。
    pub fn import_from_claude_cometix(state: &AppState) -> Result<usize, AppError> {
        Self::migrate_legacy_cometix_rows(state)?;
        let mut temp_config = crate::app_config::MultiAppConfig::default();
        let count = crate::mcp::import_from_claude_cometix(&mut temp_config)?;
        let mut new_count = 0;

        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    let storage_id = ClaudeMcpScope::Cometix.storage_id(&server.id);
                    let to_save = if let Some(existing_server) = existing.get(&storage_id) {
                        let mut merged = existing_server.clone();
                        merged.apps.claude = false;
                        merged.apps.claude_cometix = true;
                        merged.server = server.server.clone();
                        merged
                    } else {
                        new_count += 1;
                        let mut imported = server.clone();
                        imported.id = storage_id;
                        imported
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save);
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Codex 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_codex(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_codex(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Codex，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.codex = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Gemini 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_gemini(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp.rs）
        let count = crate::mcp::import_from_gemini(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Gemini，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.gemini = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Grok Build 的 `[mcp_servers]` 导入 MCP。
    pub fn import_from_grokbuild(state: &AppState) -> Result<usize, AppError> {
        let mut temp_config = crate::app_config::MultiAppConfig::default();
        let count = crate::mcp::import_from_grokbuild(&mut temp_config)?;
        let mut new_count = 0;

        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.grokbuild = true;
                        merged
                    } else {
                        new_count += 1;
                        server.clone()
                    };
                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save);
                }
            }
        }
        Ok(new_count)
    }

    /// 从 OpenCode 导入 MCP（v3.9.2+ 新增）
    pub fn import_from_opencode(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用原有的导入逻辑（从 mcp/opencode.rs）
        let count = crate::mcp::import_from_opencode(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 OpenCode，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.opencode = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Hermes 导入 MCP
    pub fn import_from_hermes(state: &AppState) -> Result<usize, AppError> {
        // 创建临时 MultiAppConfig 用于导入
        let mut temp_config = crate::app_config::MultiAppConfig::default();

        // 调用导入逻辑（从 mcp/hermes.rs）
        let count = crate::mcp::import_from_hermes(&mut temp_config)?;

        let mut new_count = 0;

        // 如果有导入的服务器，保存到数据库
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    // 已存在：仅启用 Hermes，不覆盖其他字段（与导入模块语义保持一致）
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        let mut merged = existing_server.clone();
                        merged.apps.hermes = true;
                        merged
                    } else {
                        // 真正的新服务器
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save.clone());

                    // 导入是读取已有配置，不应反向写回任何应用的 live 配置。
                    // 显式编辑、启用/禁用或手动同步时再执行写回。
                }
            }
        }

        Ok(new_count)
    }

    /// 从所有支持 MCP 的应用导入服务器，返回新导入的数量。
    ///
    /// Best-effort：单个应用导入失败（如坏 config.toml）不阻断其余应用；
    /// 全部跑完后若有失败，聚合成一个错误上报——历史实现逐应用
    /// `unwrap_or(0)` 吞错，坏文件只会表现为"导入成功 0 个"，用户
    /// 无从得知哪个应用出了问题。
    pub fn import_from_all_apps(state: &AppState) -> Result<usize, AppError> {
        let mut total = 0;
        let mut failures: Vec<String> = Vec::new();

        let results: [(&str, Result<usize, AppError>); 7] = [
            ("claude", Self::import_from_claude(state)),
            ("claude-cometix", Self::import_from_claude_cometix(state)),
            ("codex", Self::import_from_codex(state)),
            ("gemini", Self::import_from_gemini(state)),
            ("grokbuild", Self::import_from_grokbuild(state)),
            ("opencode", Self::import_from_opencode(state)),
            ("hermes", Self::import_from_hermes(state)),
        ];
        for (app, result) in results {
            match result {
                Ok(count) => total += count,
                Err(err) => {
                    log::warn!("从 {app} 导入 MCP 失败: {err}");
                    failures.push(format!("{app}: {err}"));
                }
            }
        }

        if failures.is_empty() {
            Ok(total)
        } else {
            Err(AppError::Message(format!(
                "已导入 {total} 个，部分应用导入失败: {}",
                failures.join("; ")
            )))
        }
    }
}
