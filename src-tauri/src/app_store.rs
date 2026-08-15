use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

/// Store 中的键名
const STORE_KEY_APP_CONFIG_DIR: &str = "app_config_dir_override";

/// 缓存当前的 app_config_dir 覆盖路径，避免存储 AppHandle
static APP_CONFIG_DIR_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn override_cache() -> &'static RwLock<Option<PathBuf>> {
    APP_CONFIG_DIR_OVERRIDE.get_or_init(|| RwLock::new(None))
}

fn update_cached_override(value: Option<PathBuf>) {
    if let Ok(mut guard) = override_cache().write() {
        *guard = value;
    }
}

/// 获取缓存中的 app_config_dir 覆盖路径
pub fn get_app_config_dir_override() -> Option<PathBuf> {
    override_cache().read().ok()?.clone()
}

fn read_override_from_store(app: &tauri::AppHandle) -> Option<PathBuf> {
    let store = match app.store_builder("app_paths.json").build() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("无法创建 Store: {e}");
            return None;
        }
    };

    match store.get(STORE_KEY_APP_CONFIG_DIR) {
        Some(Value::String(path_str)) => {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                return None;
            }

            let path = resolve_path(path_str);

            if let Err(err) = validate_app_config_dir_override_path(&path) {
                log::error!("忽略不安全的 app_config_dir 覆盖: {err}");
                return None;
            }

            if !path.exists() {
                log::warn!(
                    "Store 中配置的 app_config_dir 不存在: {path:?}\n\
                     将使用默认路径。"
                );
                return None;
            }

            log::info!("使用 Store 中的 app_config_dir: {path:?}");
            Some(path)
        }
        Some(_) => {
            log::warn!("Store 中的 {STORE_KEY_APP_CONFIG_DIR} 类型不正确，应为字符串");
            None
        }
        None => None,
    }
}

/// 从 Store 刷新 app_config_dir 覆盖值并更新缓存
pub fn refresh_app_config_dir_override(app: &tauri::AppHandle) -> Option<PathBuf> {
    let value = read_override_from_store(app);
    update_cached_override(value.clone());
    value
}

/// 写入 app_config_dir 到 Tauri Store
pub fn set_app_config_dir_to_store(
    app: &tauri::AppHandle,
    path: Option<&str>,
) -> Result<(), AppError> {
    let store = app
        .store_builder("app_paths.json")
        .build()
        .map_err(|e| AppError::Message(format!("创建 Store 失败: {e}")))?;

    let requested_override = path.map(str::trim).filter(|value| !value.is_empty());
    let next_app_root = requested_override
        .map(resolve_path)
        .unwrap_or_else(crate::config::get_default_app_config_dir);
    validate_app_config_dir_override_path(&next_app_root)?;
    crate::settings::validate_app_config_dir_against_current_claude_directories(&next_app_root)?;

    match requested_override {
        Some(trimmed) => {
            store.set(STORE_KEY_APP_CONFIG_DIR, Value::String(trimmed.to_string()));
            log::info!("已将 app_config_dir 写入 Store: {trimmed}");
        }
        None => {
            store.delete(STORE_KEY_APP_CONFIG_DIR);
            log::info!("已从 Store 中删除 app_config_dir 配置");
        }
    }

    store
        .save()
        .map_err(|e| AppError::Message(format!("保存 Store 失败: {e}")))?;

    refresh_app_config_dir_override(app);
    Ok(())
}

/// 解析路径，支持 ~ 开头的相对路径
pub(crate) fn resolve_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return crate::config::get_home_dir();
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        return crate::config::get_home_dir().join(stripped);
    } else if let Some(stripped) = raw.strip_prefix("~\\") {
        return crate::config::get_home_dir().join(stripped);
    }

    PathBuf::from(raw)
}

fn lexical_absolute(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| crate::config::get_home_dir())
            .join(path)
    };

    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

pub(crate) fn comparable_path(path: &Path) -> PathBuf {
    let lexical = lexical_absolute(path);
    std::fs::canonicalize(&lexical).unwrap_or(lexical)
}

fn comparison_key(path: &Path) -> String {
    let key = comparable_path(path).to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    {
        let key = key.to_ascii_lowercase();
        // `canonicalize` adds an extended-length prefix to an existing path,
        // while a not-yet-created child falls back to a normal lexical path.
        // Remove that representation-only difference before containment
        // checks, including the UNC form (`\\?\UNC\server\share`).
        if let Some(unc) = key.strip_prefix("//?/unc/") {
            format!("//{unc}")
        } else {
            key.strip_prefix("//?/").unwrap_or(&key).to_string()
        }
    }
    #[cfg(not(windows))]
    {
        key
    }
}

#[cfg(test)]
pub(crate) fn paths_equivalent(left: &Path, right: &Path) -> bool {
    comparison_key(left).trim_end_matches('/') == comparison_key(right).trim_end_matches('/')
}

pub(crate) fn path_is_same_or_nested(candidate: &Path, protected_root: &Path) -> bool {
    let candidate = comparison_key(candidate);
    let root = comparison_key(protected_root);
    let candidate = candidate.trim_end_matches('/');
    let root = root.trim_end_matches('/');
    candidate == root
        || candidate
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(crate) fn paths_overlap(left: &Path, right: &Path) -> bool {
    path_is_same_or_nested(left, right) || path_is_same_or_nested(right, left)
}

fn validate_app_config_dir_override_path(path: &Path) -> Result<(), AppError> {
    let home = crate::config::get_home_dir();
    let protected_roots = [
        home.join(".cc-switch"),
        home.join(".claude"),
        home.join(".hlclaude"),
    ];
    if protected_roots
        .iter()
        .any(|protected| paths_overlap(path, protected))
    {
        return Err(AppError::localized(
            "settings.app_config_dir.protected_path_conflict",
            "魔改版应用目录不能与原版 CC Switch、官方 Claude 或 Cometix Claude 的配置目录重合或相互嵌套",
            "The CC Switch Cometix app directory cannot overlap the upstream CC Switch, official Claude, or Cometix Claude configuration directories.",
        ));
    }
    Ok(())
}

/// 从旧的 settings.json 迁移 app_config_dir 到 Store
pub fn migrate_app_config_dir_from_settings(app: &tauri::AppHandle) -> Result<(), AppError> {
    // app_config_dir 已从 settings.json 移除，此函数保留但不再执行迁移
    // 如果用户在旧版本设置过 app_config_dir，需要在 Store 中手动配置
    log::info!("app_config_dir 迁移功能已移除，请在设置中重新配置");

    let _ = refresh_app_config_dir_override(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{path_is_same_or_nested, paths_equivalent, paths_overlap};

    #[test]
    fn protected_root_comparison_rejects_exact_and_nested_paths() {
        let root = std::path::Path::new("C:/Users/test/.cc-switch");
        assert!(path_is_same_or_nested(root, root));
        assert!(path_is_same_or_nested(
            std::path::Path::new("C:/Users/test/.cc-switch/fork"),
            root
        ));
        assert!(!path_is_same_or_nested(
            std::path::Path::new("C:/Users/test/.cc-switch-cometix"),
            root
        ));
    }

    #[test]
    fn equivalent_path_comparison_normalizes_dot_segments() {
        let base = std::env::current_dir().expect("current dir");
        assert!(paths_equivalent(&base.join("folder/.."), &base));
    }

    #[test]
    fn protected_path_overlap_is_bidirectional() {
        let live = std::path::Path::new("C:/Users/test/.claude");
        assert!(paths_overlap(
            std::path::Path::new("C:/Users/test/.claude/app-data"),
            live
        ));
        assert!(paths_overlap(std::path::Path::new("C:/Users/test"), live));
        assert!(!paths_overlap(
            std::path::Path::new("C:/Users/test/.cc-switch-cometix"),
            live
        ));
    }

    #[test]
    fn existing_root_overlaps_not_yet_created_child() {
        let root = tempfile::tempdir().expect("temp root");
        assert!(paths_overlap(
            root.path(),
            &root.path().join("future/child")
        ));
    }
}
