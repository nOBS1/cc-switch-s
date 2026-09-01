use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::skill::{SkillStorageLocation, SyncMethod};

/// 自定义端点配置（历史兼容，实际存储在 provider.meta.custom_endpoints）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpoint {
    pub url: String,
    pub added_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
}

fn default_true() -> bool {
    true
}

/// 主页面显示的应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleApps {
    #[serde(default = "default_true")]
    pub claude: bool,
    #[serde(
        rename = "claude-cometix",
        alias = "claudeCometix",
        alias = "claude_cometix",
        default = "default_true"
    )]
    pub claude_cometix: bool,
    #[serde(
        rename = "claude-desktop",
        alias = "claudeDesktop",
        alias = "claude_desktop",
        default,
        skip_deserializing
    )]
    pub claude_desktop: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
    #[serde(default = "default_true")]
    pub gemini: bool,
    #[serde(default = "default_true")]
    pub grokbuild: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
    #[serde(default = "default_true")]
    pub openclaw: bool,
    #[serde(default)]
    pub hermes: bool,
    #[serde(default = "default_true")]
    pub pi: bool,
}

impl Default for VisibleApps {
    fn default() -> Self {
        Self {
            claude: true,
            claude_cometix: true,
            claude_desktop: false,
            codex: true,
            gemini: true,
            grokbuild: true,
            opencode: true,
            openclaw: true,
            hermes: false, // 默认不显示，需用户手动启用
            pi: true,
        }
    }
}

impl VisibleApps {
    /// Check if the specified app is visible
    pub fn is_visible(&self, app: &AppType) -> bool {
        match app {
            AppType::Claude => self.claude,
            AppType::ClaudeCometix => self.claude_cometix,
            AppType::ClaudeDesktop => false,
            AppType::Codex => self.codex,
            AppType::Gemini => self.gemini,
            AppType::GrokBuild => self.grokbuild,
            AppType::OpenCode => self.opencode,
            AppType::OpenClaw => self.openclaw,
            AppType::Hermes => self.hermes,
            AppType::Pi => self.pi,
        }
    }
}

/// WebDAV 同步状态（持久化同步进度信息）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_remote_etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_local_manifest_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_remote_manifest_hash: Option<String>,
}

fn default_remote_root() -> String {
    "cc-switch-cometix-sync".to_string()
}
const UPSTREAM_DEFAULT_REMOTE_ROOT: &str = "cc-switch-sync";
fn default_profile() -> String {
    "default".to_string()
}

/// WebDAV 同步设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub status: WebDavSyncStatus,
}

impl Default for WebDavSyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_sync: false,
            base_url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_root: default_remote_root(),
            profile: default_profile(),
            status: WebDavSyncStatus::default(),
        }
    }
}

impl WebDavSyncSettings {
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        if self.base_url.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "webdav.base_url.required",
                "WebDAV 地址不能为空",
                "WebDAV URL is required.",
            ));
        }
        if self.username.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "webdav.username.required",
                "WebDAV 用户名不能为空",
                "WebDAV username is required.",
            ));
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.base_url = self.base_url.trim().to_string();
        self.username = self.username.trim().to_string();
        self.remote_root = self.remote_root.trim().to_string();
        self.profile = self.profile.trim().to_string();
        if self.remote_root.is_empty() || self.remote_root == UPSTREAM_DEFAULT_REMOTE_ROOT {
            self.remote_root = default_remote_root();
        }
        if self.profile.is_empty() {
            self.profile = default_profile();
        }
    }

    /// Returns true if all credential fields are blank (no config to persist).
    fn is_empty(&self) -> bool {
        self.base_url.is_empty() && self.username.is_empty() && self.password.is_empty()
    }
}

/// S3 同步设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3SyncSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub secret_access_key: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub status: WebDavSyncStatus,
}

impl Default for S3SyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_sync: false,
            region: String::new(),
            bucket: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            endpoint: String::new(),
            remote_root: default_remote_root(),
            profile: default_profile(),
            status: WebDavSyncStatus::default(),
        }
    }
}

impl S3SyncSettings {
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        if self.bucket.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.bucket.required",
                "S3 存储桶不能为空",
                "S3 bucket is required.",
            ));
        }
        if self.region.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.region.required",
                "S3 区域不能为空",
                "S3 region is required.",
            ));
        }
        if self.access_key_id.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.access_key_id.required",
                "S3 Access Key ID 不能为空",
                "S3 Access Key ID is required.",
            ));
        }
        if self.secret_access_key.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.secret_access_key.required",
                "S3 Secret Access Key 不能为空",
                "S3 Secret Access Key is required.",
            ));
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.region = self.region.trim().to_string();
        self.bucket = self.bucket.trim().to_string();
        self.access_key_id = self.access_key_id.trim().to_string();
        self.endpoint = self.endpoint.trim().to_string();
        self.remote_root = self.remote_root.trim().to_string();
        self.profile = self.profile.trim().to_string();
        if self.remote_root.is_empty() || self.remote_root == UPSTREAM_DEFAULT_REMOTE_ROOT {
            self.remote_root = default_remote_root();
        }
        if self.profile.is_empty() {
            self.profile = default_profile();
        }
    }

    /// Returns true if all credential fields are blank (no config to persist).
    fn is_empty(&self) -> bool {
        self.bucket.is_empty()
            && self.region.is_empty()
            && self.access_key_id.is_empty()
            && self.secret_access_key.is_empty()
    }
}

/// 本机自动迁移状态。
///
/// 这里记录的是本机启动时执行过的一次性迁移；标记不随数据库同步。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalMigrations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_third_party_history_provider_bucket_v1:
        Option<CodexThirdPartyHistoryProviderBucketMigration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_provider_template_v1: Option<CodexProviderTemplateMigration>,
    /// 统一会话开关的官方历史迁移标记。开关关闭时会被清除，
    /// 这样重新开启能把"关闭期间"落入 openai 桶的官方会话补迁进来。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_official_history_unify_v1: Option<CodexOfficialHistoryUnifyMigration>,
    /// `.claude-cometix` -> `.hlclaude` 本机文件迁移标记。
    ///
    /// 目标目录参与标记，切换 Cometix override 后新目录仍会执行迁移。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cometix_hlclaude_live_v2: Option<CometixHlclaudeLiveMigration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CometixHlclaudeLiveMigration {
    pub completed_at: String,
    pub target_config_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThirdPartyHistoryProviderBucketMigration {
    pub completed_at: String,
    pub target_provider_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_provider_ids: Vec<String>,
    #[serde(default)]
    pub migrated_jsonl_files: usize,
    #[serde(default)]
    pub migrated_state_rows: usize,
    #[serde(default)]
    pub scanned_history_files: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderTemplateMigration {
    pub completed_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrated_provider_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOfficialHistoryUnifyMigration {
    pub completed_at: String,
    pub target_provider_id: String,
    #[serde(default)]
    pub migrated_jsonl_files: usize,
    #[serde(default)]
    pub migrated_state_rows: usize,
    /// 迁移时的规范化 Codex 目录。标记只对同一目录生效：
    /// 切换 codex_config_dir 后旧标记不会挡住新目录的迁移。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_config_dir: Option<String>,
}

/// 应用设置结构
///
/// 存储设备级别设置，保存在本地
/// `~/.cc-switch-cometix/settings.json`，不随数据库同步。
/// 这确保了云同步场景下多设备可以独立运作。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    // ===== 设备级 UI 设置 =====
    #[serde(default = "default_show_in_tray")]
    pub show_in_tray: bool,
    #[serde(default = "default_minimize_to_tray_on_close")]
    pub minimize_to_tray_on_close: bool,
    #[serde(default)]
    pub use_app_window_controls: bool,
    /// 是否启用 Claude 插件联动
    #[serde(default)]
    pub enable_claude_plugin_integration: bool,
    /// 是否跳过 Claude Code 初次安装确认
    #[serde(default)]
    pub skip_claude_onboarding: bool,
    /// 是否开机自启
    #[serde(default)]
    pub launch_on_startup: bool,
    /// 静默启动（程序启动时不显示主窗口，仅托盘运行）
    #[serde(default)]
    pub silent_startup: bool,
    /// 是否在主页面启用本地代理功能（默认关闭）
    #[serde(default)]
    pub enable_local_proxy: bool,
    /// User has confirmed the local proxy first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_confirmed: Option<bool>,
    /// User has confirmed the usage query first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_dashboard_refresh_interval_ms: Option<u32>,
    /// 会话用量自动扫描开关（默认开启=自动模式）。关闭后停止后台定时扫描
    /// 各客户端会话日志，仅在用户点击"立即同步"时手动扫描；只管扫描时机，
    /// 代理接管记账与启动费用回填（不读会话文件）不受此开关影响。
    #[serde(default = "default_session_auto_sync_enabled")]
    pub session_auto_sync_enabled: bool,
    /// Whether to show the failover toggle independently on the main page
    #[serde(default)]
    pub enable_failover_toggle: bool,
    /// Whether to show the project profile switcher on the main page header
    #[serde(default = "default_show_profile_switcher")]
    pub show_profile_switcher: bool,
    /// Keep Codex ChatGPT login material in auth.json when switching to third-party providers.
    /// Opt-in: defaults to false so third-party switches cleanly overwrite auth.json.
    #[serde(default)]
    pub preserve_codex_official_auth_on_switch: bool,
    /// Run official Codex providers under the shared "custom" model_provider id
    /// so official sessions share one resume-history bucket with third-party
    /// providers. Opt-in: defaults to false.
    #[serde(default)]
    pub unify_codex_session_history: bool,
    /// User opted in (via the enable dialog checkbox) to migrate existing
    /// official sessions ("openai" bucket) into the shared bucket. Persisted so
    /// a failed migration retries at startup; cleared when the toggle turns off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unify_codex_migrate_existing: Option<bool>,
    /// User has confirmed the failover toggle first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failover_confirmed: Option<bool>,
    /// User has confirmed the first-run welcome notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_run_notice_confirmed: Option<bool>,
    /// User has confirmed the common config first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_config_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    // ===== 主页面显示的应用 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_apps: Option<VisibleApps>,

    // ===== 设备级目录覆盖 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_config_dir: Option<String>,
    /// Cometix Claude Code 的独立配置目录；不得复用官方 Claude override。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_cometix_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openclaw_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hermes_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_config_dir: Option<String>,

    // ===== 当前供应商 ID（设备级）=====
    /// 当前 Claude 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude: Option<String>,
    /// 当前 Cometix Claude Code 供应商 ID（独立于官方 Claude）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude_cometix: Option<String>,
    /// 当前 Claude Desktop 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude_desktop: Option<String>,
    /// 当前 Codex 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_codex: Option<String>,
    /// 当前 Gemini 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_gemini: Option<String>,
    /// 当前 Grok Build 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_grokbuild: Option<String>,
    /// 当前 OpenCode 供应商 ID（本地存储，对 OpenCode 可能无意义，但保持结构一致）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_opencode: Option<String>,
    /// 当前 OpenClaw 供应商 ID（本地存储，对 OpenClaw 可能无意义，但保持结构一致）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_openclaw: Option<String>,
    /// 当前 Hermes 供应商 ID（本地存储，保持结构一致）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_hermes: Option<String>,

    // ===== Skill 同步设置 =====
    /// Skill 同步方式：auto（默认，优先 symlink）、symlink、copy
    #[serde(default)]
    pub skill_sync_method: SyncMethod,
    /// Skill 存储位置：cc_switch（默认）或 unified（~/.agents/skills/）
    #[serde(default)]
    pub skill_storage_location: SkillStorageLocation,

    // ===== WebDAV 同步设置 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webdav_sync: Option<WebDavSyncSettings>,

    // ===== S3 同步设置 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3_sync: Option<S3SyncSettings>,

    // ===== WebDAV 备份设置（旧版，保留向后兼容）=====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webdav_backup: Option<serde_json::Value>,

    // ===== 备份策略设置 =====
    /// Auto-backup interval in hours (default 24, 0 = disabled)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_interval_hours: Option<u32>,
    /// Maximum number of backup files to retain (default 10)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_retain_count: Option<u32>,

    // ===== 终端设置 =====
    /// 首选终端应用（可选，默认使用系统默认终端）
    /// - macOS: "terminal" | "iterm2" | "warp" | "alacritty" | "kitty" | "ghostty" | "otty" | "wezterm" | "kaku"
    /// - Windows: "cmd" | "powershell" | "wt" (Windows Terminal)
    /// - Linux: "gnome-terminal" | "konsole" | "xfce4-terminal" | "alacritty" | "kitty" | "ghostty"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_terminal: Option<String>,

    // ===== 本机自动迁移状态 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_migrations: Option<LocalMigrations>,
}

fn default_show_in_tray() -> bool {
    true
}

fn default_minimize_to_tray_on_close() -> bool {
    true
}

fn default_show_profile_switcher() -> bool {
    true
}

fn default_session_auto_sync_enabled() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_in_tray: true,
            minimize_to_tray_on_close: true,
            use_app_window_controls: false,
            enable_claude_plugin_integration: false,
            skip_claude_onboarding: false,
            launch_on_startup: false,
            silent_startup: false,
            enable_local_proxy: false,
            proxy_confirmed: None,
            usage_confirmed: None,
            usage_dashboard_refresh_interval_ms: None,
            session_auto_sync_enabled: true,
            enable_failover_toggle: false,
            show_profile_switcher: true,
            preserve_codex_official_auth_on_switch: false,
            unify_codex_session_history: false,
            unify_codex_migrate_existing: None,
            failover_confirmed: None,
            first_run_notice_confirmed: None,
            common_config_confirmed: None,
            language: None,
            visible_apps: None,
            claude_config_dir: None,
            claude_cometix_config_dir: None,
            codex_config_dir: None,
            gemini_config_dir: None,
            grok_config_dir: None,
            opencode_config_dir: None,
            openclaw_config_dir: None,
            hermes_config_dir: None,
            pi_config_dir: None,
            current_provider_claude: None,
            current_provider_claude_cometix: None,
            current_provider_claude_desktop: None,
            current_provider_codex: None,
            current_provider_gemini: None,
            current_provider_grokbuild: None,
            current_provider_opencode: None,
            current_provider_openclaw: None,
            current_provider_hermes: None,
            skill_sync_method: SyncMethod::default(),
            skill_storage_location: SkillStorageLocation::default(),
            webdav_sync: None,
            s3_sync: None,
            webdav_backup: None,
            backup_interval_hours: None,
            backup_retain_count: None,
            preferred_terminal: None,
            local_migrations: None,
        }
    }
}

impl AppSettings {
    fn settings_path() -> Option<PathBuf> {
        // settings.json 保留用于旧版本迁移和无数据库场景
        Some(crate::config::get_app_config_dir().join("settings.json"))
    }

    fn normalize_paths(&mut self) {
        if matches!(self.skill_storage_location, SkillStorageLocation::Unified) {
            log::warn!(
                "私有版不再使用共享的 ~/.agents/skills；Skill 存储已切回独立的 CC Switch Cometix 目录"
            );
            self.skill_storage_location = SkillStorageLocation::CcSwitch;
        }

        self.claude_config_dir = self
            .claude_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.claude_cometix_config_dir = self
            .claude_cometix_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        if validate_claude_directory_isolation(self).is_err() {
            log::error!(
                "检测到 Claude/Cometix 配置目录相互重合或侵入应用数据目录，已清除自定义目录；若默认目录仍冲突，Cometix 写操作将保持禁用"
            );
            self.claude_config_dir = None;
            self.claude_cometix_config_dir = None;
        }

        self.codex_config_dir = self
            .codex_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.gemini_config_dir = self
            .gemini_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.grok_config_dir = self
            .grok_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.opencode_config_dir = self
            .opencode_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.openclaw_config_dir = self
            .openclaw_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.hermes_config_dir = self
            .hermes_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.pi_config_dir = self
            .pi_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.language = self
            .language
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| matches!(*s, "en" | "zh" | "zh-TW" | "ja"))
            .map(|s| s.to_string());

        if let Some(sync) = &mut self.webdav_sync {
            sync.normalize();
            if sync.is_empty() {
                self.webdav_sync = None;
            }
        }

        if let Some(s3) = &mut self.s3_sync {
            s3.normalize();
            if s3.is_empty() {
                self.s3_sync = None;
            }
        }
    }

    fn load_from_file() -> Self {
        let Some(path) = Self::settings_path() else {
            return Self::default();
        };
        if let Ok(content) = fs::read_to_string(&path) {
            match serde_json::from_str::<AppSettings>(&content) {
                Ok(mut settings) => {
                    settings.normalize_paths();
                    settings
                }
                Err(err) => {
                    log::warn!(
                        "解析设置文件失败，将使用默认设置。路径: {}, 错误: {}",
                        path.display(),
                        err
                    );
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }
}

fn effective_claude_directory(raw: Option<&String>, default_folder: &str) -> PathBuf {
    raw.map(|value| crate::app_store::resolve_path(value.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| crate::config::get_home_dir().join(default_folder))
}

fn effective_claude_directories(settings: &AppSettings) -> (PathBuf, PathBuf) {
    let official = effective_claude_directory(settings.claude_config_dir.as_ref(), ".claude");
    let cometix =
        effective_claude_directory(settings.claude_cometix_config_dir.as_ref(), ".hlclaude");
    (official, cometix)
}

fn validate_claude_directories_against_app_root(
    settings: &AppSettings,
    app_root: &Path,
) -> Result<(), AppError> {
    let (official, cometix) = effective_claude_directories(settings);
    if [&official, &cometix]
        .into_iter()
        .any(|claude_dir| crate::app_store::paths_overlap(claude_dir, app_root))
    {
        return Err(AppError::localized(
            "settings.claude_dir.app_data_conflict",
            "Claude 与 Cometix 的配置目录不能与 CC Switch 应用数据目录重合或相互嵌套",
            "Claude and Cometix configuration directories cannot overlap a CC Switch application data directory.",
        ));
    }
    Ok(())
}

fn validate_claude_directory_isolation_with_wsl_home(
    settings: &AppSettings,
    home: &Path,
    wsl_home: Option<&Path>,
    desktop_roots: Vec<PathBuf>,
) -> Result<(), AppError> {
    let (official, cometix) = effective_claude_directories(settings);
    let default_official = home.join(".claude");
    if crate::app_store::paths_overlap(&official, &cometix)
        || crate::app_store::paths_overlap(&default_official, &cometix)
    {
        return Err(AppError::localized(
            "settings.claude_cometix_dir.conflict",
            "Claude Code（Cometix）的配置目录不能与官方 Claude Code 相同或相互嵌套",
            "Claude Code (Cometix) and official Claude Code configuration directories cannot be the same or nested inside each other.",
        ));
    }
    validate_claude_directories_against_app_root(settings, &home.join(".cc-switch"))?;
    validate_claude_directories_against_app_root(settings, &crate::config::get_app_config_dir())?;
    let protected_roots = crate::fork_policy::cometix_protected_roots_with_wsl_home(
        home,
        &official,
        wsl_home,
        desktop_roots,
    );
    crate::fork_policy::ensure_cometix_paths_isolated(&official, &cometix, &protected_roots)?;
    Ok(())
}

pub(crate) fn validate_claude_directory_isolation(settings: &AppSettings) -> Result<(), AppError> {
    let home = crate::config::get_home_dir();
    let desktop_roots =
        crate::claude_desktop_config::get_protected_config_roots().unwrap_or_default();
    validate_claude_directory_isolation_with_wsl_home(settings, &home, None, desktop_roots)?;

    // The settings write is a mutation boundary. A WSL UNC override must be
    // probed and inspected inside its distro here, before it can be persisted;
    // host-side UNC canonicalization cannot reliably follow Linux symlinks.
    let (official, cometix) = effective_claude_directories(settings);
    crate::fork_policy::ensure_cometix_config_paths_isolated(&official, &cometix)?;
    Ok(())
}

/// Runtime guard for changing the fork's app-data root. The Store startup
/// reader intentionally cannot consult settings (initialization order); the
/// interactive setter can and must protect the currently live custom Claude
/// directories before switching roots.
pub(crate) fn validate_app_config_dir_against_current_claude_directories(
    app_root: &Path,
) -> Result<(), AppError> {
    validate_claude_directories_against_app_root(&get_settings(), app_root)
}

fn save_settings_file(settings: &AppSettings) -> Result<(), AppError> {
    let mut normalized = settings.clone();
    normalized.normalize_paths();
    let Some(path) = AppSettings::settings_path() else {
        return Err(AppError::Config("无法获取用户主目录".to_string()));
    };
    crate::app_store::ensure_private_app_data_path_isolated(&path)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let json = serde_json::to_string_pretty(&normalized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| AppError::io(&path, e))?;
        file.write_all(json.as_bytes())
            .map_err(|e| AppError::io(&path, e))?;
    }

    #[cfg(not(unix))]
    {
        fs::write(&path, json).map_err(|e| AppError::io(&path, e))?;
    }

    Ok(())
}

static SETTINGS_STORE: OnceLock<RwLock<AppSettings>> = OnceLock::new();

fn settings_store() -> &'static RwLock<AppSettings> {
    SETTINGS_STORE.get_or_init(|| RwLock::new(AppSettings::load_from_file()))
}

pub(crate) fn resolve_override_path(raw: &str) -> PathBuf {
    let join_home = |home: PathBuf, suffix: &str| {
        suffix
            .split(['/', '\\'])
            .filter(|component| !component.is_empty())
            .fold(home, |path, component| path.join(component))
    };

    if raw == "~" {
        return crate::config::get_home_dir();
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        return join_home(crate::config::get_home_dir(), stripped);
    } else if let Some(stripped) = raw.strip_prefix("~\\") {
        return join_home(crate::config::get_home_dir(), stripped);
    }

    PathBuf::from(raw)
}

pub fn get_settings() -> AppSettings {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .clone()
}

pub fn get_settings_for_frontend() -> AppSettings {
    let mut settings = get_settings();
    if let Some(sync) = &mut settings.webdav_sync {
        sync.password.clear();
    }
    if let Some(s3) = &mut settings.s3_sync {
        s3.secret_access_key.clear();
    }
    settings.webdav_backup = None;
    settings
}

pub fn update_settings(mut new_settings: AppSettings) -> Result<(), AppError> {
    validate_claude_directory_isolation(&new_settings)?;
    ensure_private_skill_storage_location(new_settings.skill_storage_location)?;
    new_settings.normalize_paths();
    save_settings_file(&new_settings)?;

    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    *guard = new_settings;
    Ok(())
}

fn mutate_settings<F>(mutator: F) -> Result<(), AppError>
where
    F: FnOnce(&mut AppSettings),
{
    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    let mut next = guard.clone();
    mutator(&mut next);
    next.normalize_paths();
    save_settings_file(&next)?;
    *guard = next;
    Ok(())
}

pub fn is_codex_third_party_history_provider_bucket_migrated() -> bool {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| {
            migrations
                .codex_third_party_history_provider_bucket_v1
                .as_ref()
        })
        .is_some_and(|m| m.scanned_history_files)
}

pub fn mark_codex_third_party_history_provider_bucket_migrated(
    migration: CodexThirdPartyHistoryProviderBucketMigration,
) -> Result<(), AppError> {
    mutate_settings(|settings| {
        let migrations = settings
            .local_migrations
            .get_or_insert_with(Default::default);
        migrations.codex_third_party_history_provider_bucket_v1 = Some(migration);
    })
}

pub fn is_codex_provider_template_migrated() -> bool {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.codex_provider_template_v1.as_ref())
        .is_some()
}

pub fn mark_codex_provider_template_migrated(
    migration: CodexProviderTemplateMigration,
) -> Result<(), AppError> {
    mutate_settings(|settings| {
        let migrations = settings
            .local_migrations
            .get_or_insert_with(Default::default);
        migrations.codex_provider_template_v1 = Some(migration);
    })
}

/// 获取 `.hlclaude` 本机迁移标记。
pub fn get_cometix_hlclaude_live_v2_migration() -> Option<CometixHlclaudeLiveMigration> {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.cometix_hlclaude_live_v2.clone())
}

/// 标记指定 Cometix 配置目录已完成本机迁移。
pub fn mark_cometix_hlclaude_live_v2_migrated(target_config_dir: &Path) -> Result<(), AppError> {
    let target_config_dir = target_config_dir.to_string_lossy().to_string();
    mutate_settings(|settings| {
        let migrations = settings
            .local_migrations
            .get_or_insert_with(Default::default);
        migrations.cometix_hlclaude_live_v2 = Some(CometixHlclaudeLiveMigration {
            completed_at: chrono::Utc::now().to_rfc3339(),
            target_config_dir,
        });
    })
}

/// 统一会话迁移标记是否覆盖指定目录。标记里没记目录（不应出现的旧格式）
/// 视为不匹配——重跑迁移是幂等的，宁可重迁也不漏迁。
pub fn is_codex_official_history_unify_migrated_for_dir(codex_dir: &str) -> bool {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.codex_official_history_unify_v1.as_ref())
        .is_some_and(|migration| migration.codex_config_dir.as_deref() == Some(codex_dir))
}

/// 条件写入迁移完成标记：仅当此刻开关仍开启且迁移意愿仍在时才写。
/// 检查与写入在 settings 写锁内原子完成，与关闭开关路径
/// （`update_settings` / 清标记）串行，消除"迁移线程复查开关后、写标记前
/// 用户恰好关闭开关"的竞态窗口。返回是否实际写入。
pub fn mark_codex_official_history_unify_migrated_if_enabled(
    migration: CodexOfficialHistoryUnifyMigration,
) -> Result<bool, AppError> {
    let mut written = false;
    mutate_settings(|settings| {
        if settings.unify_codex_session_history
            && settings.unify_codex_migrate_existing.unwrap_or(false)
        {
            settings
                .local_migrations
                .get_or_insert_with(Default::default)
                .codex_official_history_unify_v1 = Some(migration);
            written = true;
        }
    })?;
    Ok(written)
}

pub fn clear_codex_official_history_unify_migration() -> Result<(), AppError> {
    mutate_settings(|settings| {
        if let Some(migrations) = settings.local_migrations.as_mut() {
            migrations.codex_official_history_unify_v1 = None;
        }
    })
}

pub fn unify_codex_migrate_existing_requested() -> bool {
    get_settings().unify_codex_migrate_existing.unwrap_or(false)
}

pub fn clear_codex_unify_migrate_existing() -> Result<(), AppError> {
    mutate_settings(|settings| {
        settings.unify_codex_migrate_existing = None;
    })
}

/// 从文件重新加载设置到内存缓存
/// 用于导入配置等场景，确保内存缓存与文件同步
pub fn reload_settings() -> Result<(), AppError> {
    let fresh_settings = AppSettings::load_from_file();
    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    *guard = fresh_settings;
    Ok(())
}

pub fn get_claude_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .claude_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_claude_cometix_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .claude_cometix_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_codex_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .codex_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_gemini_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .gemini_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_grok_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .grok_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_opencode_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .opencode_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_openclaw_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .openclaw_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_hermes_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .hermes_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_pi_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .pi_config_dir
        .as_ref()
        .map(|path| resolve_override_path(path))
}

pub fn preserve_codex_official_auth_on_switch() -> bool {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .preserve_codex_official_auth_on_switch
}

pub fn unify_codex_session_history() -> bool {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .unify_codex_session_history
}

// ===== 当前供应商管理函数 =====

/// 获取指定应用类型的当前供应商 ID（从本地 settings 读取）
///
/// 这是设备级别的设置，不随数据库同步。
/// 如果本地没有设置，调用者应该 fallback 到数据库的 `is_current` 字段。
pub fn get_current_provider(app_type: &AppType) -> Option<String> {
    let settings = settings_store().read().ok()?;
    match app_type {
        AppType::Claude => settings.current_provider_claude.clone(),
        AppType::ClaudeCometix => settings.current_provider_claude_cometix.clone(),
        AppType::ClaudeDesktop => settings.current_provider_claude_desktop.clone(),
        AppType::Codex => settings.current_provider_codex.clone(),
        AppType::Gemini => settings.current_provider_gemini.clone(),
        AppType::GrokBuild => settings.current_provider_grokbuild.clone(),
        AppType::OpenCode => settings.current_provider_opencode.clone(),
        AppType::OpenClaw => settings.current_provider_openclaw.clone(),
        AppType::Hermes => settings.current_provider_hermes.clone(),
        AppType::Pi => None,
    }
}

/// 设置指定应用类型的当前供应商 ID（保存到本地 settings）
///
/// 这是设备级别的设置，不随数据库同步。
/// 传入 `None` 会清除当前供应商设置。
pub fn set_current_provider(app_type: &AppType, id: Option<&str>) -> Result<(), AppError> {
    let id_owned = id.map(|s| s.to_string());
    mutate_settings(|settings| match app_type {
        AppType::Claude => settings.current_provider_claude = id_owned.clone(),
        AppType::ClaudeCometix => settings.current_provider_claude_cometix = id_owned.clone(),
        AppType::ClaudeDesktop => settings.current_provider_claude_desktop = id_owned.clone(),
        AppType::Codex => settings.current_provider_codex = id_owned.clone(),
        AppType::Gemini => settings.current_provider_gemini = id_owned.clone(),
        AppType::GrokBuild => settings.current_provider_grokbuild = id_owned.clone(),
        AppType::OpenCode => settings.current_provider_opencode = id_owned.clone(),
        AppType::OpenClaw => settings.current_provider_openclaw = id_owned.clone(),
        AppType::Hermes => settings.current_provider_hermes = id_owned.clone(),
        AppType::Pi => {}
    })
}

/// 获取有效的当前供应商 ID（验证存在性）
///
/// 逻辑：
/// 1. 从本地 settings 读取当前供应商 ID
/// 2. 验证该 ID 在数据库中存在
/// 3. 如果不存在则清理本地 settings，fallback 到数据库的 is_current
///
/// 这确保了返回的 ID 一定是有效的（在数据库中存在）。
/// 多设备云同步场景下，配置导入后本地 ID 可能失效，此函数会自动修复。
pub fn get_effective_current_provider(
    db: &crate::database::Database,
    app_type: &AppType,
) -> Result<Option<String>, AppError> {
    // 1. 从本地 settings 读取
    if let Some(local_id) = get_current_provider(app_type) {
        // 2. 验证该 ID 在数据库中存在
        let providers = db.get_all_providers(app_type.as_str())?;
        if providers.contains_key(&local_id) {
            // 存在，直接返回
            return Ok(Some(local_id));
        }

        // 3. 不存在，清理本地 settings
        log::warn!(
            "本地 settings 中的供应商 {} ({}) 在数据库中不存在，将清理并 fallback 到数据库",
            local_id,
            app_type.as_str()
        );
        let _ = set_current_provider(app_type, None);
    }

    // Fallback 到数据库的 is_current
    db.get_current_provider(app_type.as_str())
}

// ===== Skill 同步方式管理函数 =====

/// 获取 Skill 同步方式配置
pub fn get_skill_sync_method() -> SyncMethod {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .skill_sync_method
}

// ===== Skill 存储位置管理函数 =====

#[cfg(test)]
thread_local! {
    static TEST_SKILL_STORAGE_LOCATION: std::cell::Cell<Option<SkillStorageLocation>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn replace_test_skill_storage_location(
    location: Option<SkillStorageLocation>,
) -> Option<SkillStorageLocation> {
    TEST_SKILL_STORAGE_LOCATION.with(|slot| slot.replace(location))
}

#[cfg(test)]
fn test_skill_storage_location() -> Option<SkillStorageLocation> {
    TEST_SKILL_STORAGE_LOCATION.with(std::cell::Cell::get)
}

pub(crate) fn ensure_private_skill_storage_location(
    location: SkillStorageLocation,
) -> Result<(), AppError> {
    #[cfg(test)]
    if test_skill_storage_location().is_some() {
        return Ok(());
    }

    if matches!(location, SkillStorageLocation::Unified) {
        return Err(AppError::localized(
            "settings.skill_storage.shared_disabled",
            "此魔改版仅使用独立的 Skill 存储，不能修改官方 Claude 也会读取的 ~/.agents/skills",
            "This private build only uses isolated Skill storage and cannot modify ~/.agents/skills, which official Claude may also read.",
        ));
    }
    Ok(())
}

/// 获取 Skill 存储位置配置
pub fn get_skill_storage_location() -> SkillStorageLocation {
    #[cfg(test)]
    if let Some(location) = test_skill_storage_location() {
        return location;
    }

    let configured = settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .skill_storage_location;
    match configured {
        SkillStorageLocation::CcSwitch => SkillStorageLocation::CcSwitch,
        SkillStorageLocation::Unified => {
            log::warn!(
                "忽略共享的 Unified Skill 存储设置；私有版固定使用 CC Switch Cometix 独立目录"
            );
            SkillStorageLocation::CcSwitch
        }
    }
}

/// 设置 Skill 存储位置
pub fn set_skill_storage_location(location: SkillStorageLocation) -> Result<(), AppError> {
    ensure_private_skill_storage_location(location)?;

    #[cfg(test)]
    if test_skill_storage_location().is_some() {
        TEST_SKILL_STORAGE_LOCATION.with(|slot| slot.set(Some(location)));
        return Ok(());
    }

    mutate_settings(|s| {
        s.skill_storage_location = location;
    })
}

// ===== 备份策略管理函数 =====

/// Get the effective auto-backup interval in hours (default 24)
pub fn effective_backup_interval_hours() -> u32 {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .backup_interval_hours
        .unwrap_or(24)
}

/// Get the effective backup retain count (default 10, minimum 1)
pub fn effective_backup_retain_count() -> usize {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .backup_retain_count
        .map(|n| (n as usize).max(1))
        .unwrap_or(10)
}

// ===== 终端设置管理函数 =====

/// 获取首选终端应用
pub fn get_preferred_terminal() -> Option<String> {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .preferred_terminal
        .clone()
}

// ===== WebDAV 同步设置管理函数 =====

/// 获取 WebDAV 同步设置
pub fn get_webdav_sync_settings() -> Option<WebDavSyncSettings> {
    settings_store().read().ok()?.webdav_sync.clone()
}

/// 保存 WebDAV 同步设置
pub fn set_webdav_sync_settings(settings: Option<WebDavSyncSettings>) -> Result<(), AppError> {
    mutate_settings(|current| {
        current.webdav_sync = settings;
    })
}

/// 仅更新 WebDAV 同步状态，避免覆写 credentials/root/profile 等字段
pub fn update_webdav_sync_status(status: WebDavSyncStatus) -> Result<(), AppError> {
    mutate_settings(|current| {
        if let Some(sync) = current.webdav_sync.as_mut() {
            sync.status = status;
        }
    })
}

// ===== S3 同步设置管理函数 =====

pub fn get_s3_sync_settings() -> Option<S3SyncSettings> {
    settings_store().read().ok()?.s3_sync.clone()
}

pub fn set_s3_sync_settings(settings: Option<S3SyncSettings>) -> Result<(), AppError> {
    mutate_settings(|current| {
        current.s3_sync = settings;
    })
}

pub fn update_s3_sync_status(status: WebDavSyncStatus) -> Result<(), AppError> {
    mutate_settings(|current| {
        if let Some(s3) = current.s3_sync.as_mut() {
            s3.status = status;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;

    #[test]
    fn legacy_unified_skill_storage_normalizes_to_private_storage() {
        let mut settings = AppSettings {
            skill_storage_location: SkillStorageLocation::Unified,
            ..Default::default()
        };

        settings.normalize_paths();

        assert_eq!(
            settings.skill_storage_location,
            SkillStorageLocation::CcSwitch
        );
    }

    #[test]
    fn private_build_rejects_shared_skill_storage() {
        let previous = replace_test_skill_storage_location(None);
        let result = ensure_private_skill_storage_location(SkillStorageLocation::Unified);
        replace_test_skill_storage_location(previous);

        let error = result.expect_err("shared Skill storage must be rejected");
        assert!(error.to_string().contains("~/.agents/skills"));
    }

    #[test]
    fn cometix_directory_override_is_normalized_independently() {
        let mut settings: AppSettings = serde_json::from_value(serde_json::json!({
            "claudeConfigDir": " /profiles/official ",
            "claudeCometixConfigDir": " /profiles/cometix "
        }))
        .expect("deserialize settings");

        settings.normalize_paths();
        let serialized = serde_json::to_value(settings).expect("serialize settings");

        assert_eq!(serialized["claudeConfigDir"], "/profiles/official");
        assert_eq!(serialized["claudeCometixConfigDir"], "/profiles/cometix");
    }

    #[test]
    fn claude_and_cometix_directory_aliases_are_rejected() {
        let temp = tempfile::tempdir().expect("temp dir");
        let shared = temp.path().join("shared");
        std::fs::create_dir_all(&shared).expect("create shared dir");
        let settings = AppSettings {
            claude_config_dir: Some(shared.to_string_lossy().to_string()),
            claude_cometix_config_dir: Some(shared.join(".").to_string_lossy().to_string()),
            ..Default::default()
        };

        assert!(validate_claude_directory_isolation(&settings).is_err());
    }

    #[test]
    fn claude_and_cometix_nested_directories_are_rejected_in_both_directions() {
        let temp = tempfile::tempdir().expect("temp dir");
        let official = temp.path().join("official");
        let cometix = temp.path().join("cometix");

        let cometix_inside_official = AppSettings {
            claude_config_dir: Some(official.to_string_lossy().to_string()),
            claude_cometix_config_dir: Some(official.join("skills").to_string_lossy().to_string()),
            ..Default::default()
        };
        assert!(validate_claude_directory_isolation(&cometix_inside_official).is_err());

        let official_inside_cometix = AppSettings {
            claude_config_dir: Some(cometix.join("official").to_string_lossy().to_string()),
            claude_cometix_config_dir: Some(cometix.to_string_lossy().to_string()),
            ..Default::default()
        };
        assert!(validate_claude_directory_isolation(&official_inside_cometix).is_err());
    }

    #[test]
    fn cometix_directory_cannot_overlap_the_selected_wsl_home_official_data() {
        let temp = tempfile::tempdir().expect("temp dir");
        let host_home = temp.path().join("host");
        let wsl_home = temp.path().join("wsl-home");
        let settings = AppSettings {
            claude_config_dir: Some(host_home.join(".claude").display().to_string()),
            claude_cometix_config_dir: Some(
                wsl_home.join(".claude/projects").display().to_string(),
            ),
            ..Default::default()
        };

        assert!(validate_claude_directory_isolation_with_wsl_home(
            &settings,
            &host_home,
            Some(&wsl_home),
            Vec::new(),
        )
        .is_err());
    }

    #[test]
    fn custom_legacy_official_override_does_not_unprotect_default_claude_root() {
        let temp = tempfile::tempdir().expect("temp dir");
        let settings = AppSettings {
            claude_config_dir: Some(temp.path().join("legacy-official").display().to_string()),
            claude_cometix_config_dir: Some(
                crate::config::get_home_dir()
                    .join(".claude")
                    .display()
                    .to_string(),
            ),
            ..Default::default()
        };

        assert!(validate_claude_directory_isolation(&settings).is_err());
    }

    #[test]
    #[serial_test::serial]
    fn claude_directories_cannot_overlap_upstream_or_fork_app_data() {
        let temp = tempfile::tempdir().expect("temp dir");
        let separate_cometix = temp.path().join("cometix");

        let inside_upstream = AppSettings {
            claude_config_dir: Some(
                crate::config::get_home_dir()
                    .join(".cc-switch/claude-state")
                    .to_string_lossy()
                    .to_string(),
            ),
            claude_cometix_config_dir: Some(separate_cometix.to_string_lossy().to_string()),
            ..Default::default()
        };
        assert!(validate_claude_directory_isolation(&inside_upstream).is_err());

        let fork_root = crate::config::get_app_config_dir();
        let inside_fork = AppSettings {
            claude_config_dir: Some(
                fork_root
                    .join("official-state")
                    .to_string_lossy()
                    .to_string(),
            ),
            claude_cometix_config_dir: Some(separate_cometix.to_string_lossy().to_string()),
            ..Default::default()
        };
        assert!(
            validate_claude_directory_isolation(&inside_fork).is_err(),
            "a Claude directory inside the fork app root must be rejected"
        );
    }

    #[test]
    fn prospective_app_root_cannot_overlap_current_claude_directories() {
        let temp = tempfile::tempdir().expect("temp dir");
        let official = temp.path().join("official");
        let cometix = temp.path().join("cometix");
        let settings = AppSettings {
            claude_config_dir: Some(official.to_string_lossy().to_string()),
            claude_cometix_config_dir: Some(cometix.to_string_lossy().to_string()),
            ..Default::default()
        };

        assert!(validate_claude_directories_against_app_root(
            &settings,
            &official.join("app-data")
        )
        .is_err());
        assert!(validate_claude_directories_against_app_root(&settings, temp.path()).is_err());
        assert!(validate_claude_directories_against_app_root(
            &settings,
            &temp.path().join("separate-app-data")
        )
        .is_ok());
    }

    #[test]
    fn cloned_upstream_sync_defaults_move_to_the_fork_namespace() {
        let mut webdav = WebDavSyncSettings {
            remote_root: UPSTREAM_DEFAULT_REMOTE_ROOT.to_string(),
            ..Default::default()
        };
        webdav.normalize();
        assert_eq!(webdav.remote_root, "cc-switch-cometix-sync");

        let mut s3 = S3SyncSettings {
            remote_root: UPSTREAM_DEFAULT_REMOTE_ROOT.to_string(),
            ..Default::default()
        };
        s3.normalize();
        assert_eq!(s3.remote_root, "cc-switch-cometix-sync");

        webdav.remote_root = "my-private-root".to_string();
        webdav.normalize();
        assert_eq!(webdav.remote_root, "my-private-root");
    }

    #[test]
    fn cometix_live_migration_marker_keeps_target_directory() {
        let migrations: LocalMigrations = serde_json::from_value(serde_json::json!({
            "cometixHlclaudeLiveV2": {
                "completedAt": "2026-08-15T00:00:00Z",
                "targetConfigDir": "/profiles/cometix"
            }
        }))
        .expect("deserialize local migration marker");

        assert_eq!(
            migrations
                .cometix_hlclaude_live_v2
                .expect("Cometix marker")
                .target_config_dir,
            "/profiles/cometix"
        );
    }

    #[test]
    fn visible_apps_old_settings_default_cometix_visible_and_desktop_hidden() {
        let visible: VisibleApps = serde_json::from_value(serde_json::json!({
            "claude": true,
            "codex": true,
            "gemini": true,
            "opencode": true,
            "openclaw": true,
            "hermes": true
        }))
        .expect("visible apps");

        assert!(!visible.is_visible(&AppType::ClaudeDesktop));
        assert!(visible.claude_cometix);
    }

    #[test]
    fn visible_apps_ignores_legacy_claude_desktop_enablement() {
        let visible: VisibleApps = serde_json::from_value(serde_json::json!({
            "claude": true,
            "claudeDesktop": true,
            "codex": true,
            "gemini": true,
            "opencode": true,
            "openclaw": true,
            "hermes": true
        }))
        .expect("visible apps");

        assert!(!visible.is_visible(&AppType::ClaudeDesktop));
        assert!(!visible.claude_desktop);
    }

    #[test]
    fn visible_apps_accepts_claude_cometix_aliases() {
        let visible: VisibleApps = serde_json::from_value(serde_json::json!({
            "claude": true,
            "claudeCometix": false
        }))
        .expect("visible apps");

        assert!(!visible.claude_cometix);
    }

    #[test]
    fn override_paths_expand_windows_style_tilde_separators() {
        let home = crate::config::get_home_dir();
        assert_eq!(
            resolve_override_path(r"~\pi\agent"),
            home.join("pi").join("agent")
        );
    }
}
