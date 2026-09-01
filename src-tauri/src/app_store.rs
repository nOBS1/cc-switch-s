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
    comparable_path_inner(path, 0)
}

fn comparable_path_inner(path: &Path, link_depth: usize) -> PathBuf {
    let lexical = lexical_absolute(path);
    let mut ancestor = lexical.clone();
    let mut missing_suffix = Vec::new();

    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(&ancestor) {
            for component in missing_suffix.iter().rev() {
                resolved.push(component);
            }
            return resolved;
        }

        // `canonicalize` fails for a symlink whose target does not exist. Do
        // not then treat that link as an ordinary missing filename: resolve
        // the link text ourselves and continue from its target so a dangling
        // alias into an official/protected tree is still rejected before a
        // create/write call follows it.
        // Stay above the symlink/reparse traversal limits used by supported
        // filesystems. A chain that the eventual write can follow must not be
        // able to outlast the comparison guard and hide an official target.
        if link_depth < 128
            && std::fs::symlink_metadata(&ancestor)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            if let Ok(link_target) = std::fs::read_link(&ancestor) {
                let mut resolved_target = if link_target.is_absolute() {
                    link_target
                } else {
                    ancestor
                        .parent()
                        .unwrap_or_else(|| Path::new(""))
                        .join(link_target)
                };
                for component in missing_suffix.iter().rev() {
                    resolved_target.push(component);
                }
                return comparable_path_inner(&resolved_target, link_depth + 1);
            }
        }

        let Some(file_name) = ancestor.file_name().map(|name| name.to_os_string()) else {
            return lexical;
        };
        missing_suffix.push(file_name);
        let Some(parent) = ancestor.parent() else {
            return lexical;
        };
        ancestor = parent.to_path_buf();
    }
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

#[cfg(unix)]
pub(crate) fn same_existing_file_identity(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    enum Identity {
        MissingOrNotFile,
        Known(u64, u64),
        Unverifiable,
    }

    fn identity(path: &Path) -> Identity {
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => Identity::Known(metadata.dev(), metadata.ino()),
            Ok(_) => Identity::MissingOrNotFile,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Identity::MissingOrNotFile
            }
            Err(_) => Identity::Unverifiable,
        }
    }

    match (identity(left), identity(right)) {
        (Identity::Known(left_dev, left_ino), Identity::Known(right_dev, right_ino)) => {
            left_dev == right_dev && left_ino == right_ino
        }
        (Identity::Unverifiable, Identity::Unverifiable)
        | (Identity::Unverifiable, Identity::Known(_, _))
        | (Identity::Known(_, _), Identity::Unverifiable) => true,
        _ => false,
    }
}

#[cfg(windows)]
pub(crate) fn same_existing_file_identity(left: &Path, right: &Path) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };

    enum Identity {
        MissingOrNotFile,
        Known(u64, [u8; 16]),
        Unverifiable,
    }

    fn identity(path: &Path) -> Identity {
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Identity::MissingOrNotFile,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Identity::MissingOrNotFile;
            }
            Err(_) => return Identity::Unverifiable,
        }

        let Ok(file) = std::fs::File::open(path) else {
            return Identity::Unverifiable;
        };
        let mut information = FILE_ID_INFO::default();
        // SAFETY: `file` owns a live handle and `information` is a correctly
        // sized writable FILE_ID_INFO buffer for this synchronous call.
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                std::ptr::addr_of_mut!(information).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } != 0;
        if succeeded {
            Identity::Known(
                information.VolumeSerialNumber,
                information.FileId.Identifier,
            )
        } else {
            Identity::Unverifiable
        }
    }

    match (identity(left), identity(right)) {
        (Identity::Known(left_volume, left_id), Identity::Known(right_volume, right_id)) => {
            left_volume == right_volume && left_id == right_id
        }
        (Identity::Unverifiable, Identity::Unverifiable)
        | (Identity::Unverifiable, Identity::Known(_, _))
        | (Identity::Known(_, _), Identity::Unverifiable) => true,
        _ => false,
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn same_existing_file_identity(_left: &Path, _right: &Path) -> bool {
    false
}

#[cfg(unix)]
pub(crate) fn existing_file_has_multiple_links(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_file() && metadata.nlink() > 1,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

#[cfg(windows)]
pub(crate) fn existing_file_has_multiple_links(path: &Path) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    }

    let Ok(file) = std::fs::File::open(path) else {
        return true;
    };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a live handle and `information` is a correctly sized
    // writable structure for the synchronous Win32 query.
    let succeeded = unsafe {
        GetFileInformationByHandle(file.as_raw_handle(), std::ptr::addr_of_mut!(information))
    } != 0;
    !succeeded || information.nNumberOfLinks > 1
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn existing_file_has_multiple_links(_path: &Path) -> bool {
    false
}

fn validate_managed_app_data_path(
    path: &Path,
    private_root: &Path,
    upstream_root: &Path,
    upstream_db: &Path,
) -> Result<(), AppError> {
    let aliases_upstream_relative_file = path
        .strip_prefix(private_root)
        .ok()
        .is_some_and(|relative| same_existing_file_identity(path, &upstream_root.join(relative)));
    if !path_is_same_or_nested(path, private_root)
        || paths_overlap(path, upstream_root)
        || same_existing_file_identity(path, upstream_db)
        || aliases_upstream_relative_file
        || existing_file_has_multiple_links(path)
    {
        return Err(managed_app_data_conflict_error());
    }

    Ok(())
}

fn managed_app_data_conflict_error() -> AppError {
    AppError::localized(
        "settings.app_config_dir.managed_path_conflict",
        "魔改版应用数据路径指向了原版 CC Switch 或官方 Claude 数据，已拒绝访问以保护原版数据库和配置",
        "A private-build application-data path resolves to upstream CC Switch or official Claude data, so access was denied to protect the upstream database and configuration",
    )
}

fn ensure_private_app_data_path_isolated_with_root(
    path: &Path,
    private_root: &Path,
) -> Result<(), AppError> {
    let home = crate::config::get_home_dir();
    let upstream_root = home.join(".cc-switch");
    validate_managed_app_data_path(
        path,
        private_root,
        &upstream_root,
        &upstream_root.join("cc-switch.db"),
    )?;

    if crate::claude_desktop_config::get_protected_config_roots()
        .is_ok_and(|roots| roots.iter().any(|root| paths_overlap(path, root)))
    {
        return Err(managed_app_data_conflict_error());
    }

    let mut official_files = vec![
        home.join(".claude.json"),
        home.join(".claude").join("settings.json"),
        home.join(".claude").join("CLAUDE.md"),
    ];
    if let Ok(desktop_config) = crate::claude_desktop_config::get_config_library_path() {
        official_files.push(desktop_config);
    }
    if official_files
        .iter()
        .any(|official| same_existing_file_identity(path, official))
    {
        return Err(managed_app_data_conflict_error());
    }

    Ok(())
}

pub(crate) fn ensure_private_app_data_path_isolated(path: &Path) -> Result<(), AppError> {
    ensure_private_app_data_path_isolated_with_root(path, &crate::config::get_app_config_dir())
}

/// Test-only dependency injection for code that writes private app-data
/// backups. Each test supplies its own temporary private root, avoiding
/// process-global HOME/app-store overrides while exercising the same
/// containment, alias, hard-link, and official-data checks as production.
#[cfg(test)]
pub(crate) fn ensure_private_app_data_path_isolated_for_test(
    path: &Path,
    private_root: &Path,
) -> Result<(), AppError> {
    ensure_private_app_data_path_isolated_with_root(path, private_root)
}

fn app_config_override_protected_roots(
    home: &Path,
    wsl_home: Option<&Path>,
    mut desktop_roots: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut protected_roots = vec![
        home.join(".cc-switch"),
        home.join(".claude"),
        home.join(".claude.json"),
        home.join(".claude-desktop"),
        home.join(".hlclaude"),
        home.join(".agents").join("skills"),
    ];
    if let Some(wsl_home) = wsl_home {
        protected_roots.extend([
            wsl_home.join(".cc-switch"),
            wsl_home.join(".claude"),
            wsl_home.join(".claude.json"),
            wsl_home.join(".claude-desktop"),
            wsl_home.join(".hlclaude"),
            wsl_home.join(".agents").join("skills"),
        ]);
    }
    protected_roots.append(&mut desktop_roots);
    protected_roots.sort();
    protected_roots.dedup();
    protected_roots
}

fn app_config_dir_conflict_error() -> AppError {
    AppError::localized(
        "settings.app_config_dir.protected_path_conflict",
        "魔改版应用目录不能与原版 CC Switch、官方 Claude、Cometix Claude 或共享 Skills 的配置目录重合或相互嵌套",
        "The CC Switch Cometix app directory cannot overlap upstream CC Switch, official Claude, Cometix Claude, or shared Skills configuration directories.",
    )
}

fn validate_app_config_dir_override_path_with_context(
    path: &Path,
    home: &Path,
    wsl_home: Option<&Path>,
    desktop_roots: Vec<PathBuf>,
) -> Result<(), AppError> {
    let protected_roots = app_config_override_protected_roots(home, wsl_home, desktop_roots);
    if protected_roots
        .iter()
        .any(|protected| paths_overlap(path, protected))
    {
        return Err(app_config_dir_conflict_error());
    }
    Ok(())
}

fn validate_app_config_dir_override_path(path: &Path) -> Result<(), AppError> {
    let home = crate::config::get_home_dir();
    let wsl_home = crate::fork_policy::resolve_cometix_wsl_home(path)?;
    let desktop_roots = match crate::claude_desktop_config::get_protected_config_roots() {
        Ok(roots) => roots,
        Err(error) if wsl_home.is_some() => return Err(error),
        Err(_) => Vec::new(),
    };
    #[cfg(target_os = "windows")]
    let protected_roots =
        app_config_override_protected_roots(&home, wsl_home.as_deref(), desktop_roots.clone());
    validate_app_config_dir_override_path_with_context(
        path,
        &home,
        wsl_home.as_deref(),
        desktop_roots,
    )?;

    #[cfg(target_os = "windows")]
    if let Some(distro) = crate::fork_policy::wsl_distro_from_unc_path(path) {
        crate::fork_policy::ensure_wsl_tree_isolated(&distro, path, &protected_roots, true)?;
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
    use super::{
        path_is_same_or_nested, paths_equivalent, paths_overlap,
        validate_app_config_dir_override_path_with_context, validate_managed_app_data_path,
    };

    #[cfg(unix)]
    fn alias_directory(source: &std::path::Path, destination: &std::path::Path) -> bool {
        std::os::unix::fs::symlink(source, destination).is_ok()
    }

    #[cfg(windows)]
    fn alias_directory(source: &std::path::Path, destination: &std::path::Path) -> bool {
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
    fn app_config_override_rejects_host_and_wsl_official_or_shared_roots() {
        let temp = tempfile::tempdir().expect("temp root");
        let host_home = temp.path().join("host-home");
        let wsl_home = temp.path().join("wsl-home");
        let desktop_root = temp.path().join("windows-claude-desktop");

        for protected in [
            host_home.join(".cc-switch"),
            host_home.join(".claude/projects"),
            host_home.join(".agents/skills/private-skill"),
            host_home.join(".agents"),
            wsl_home.join(".cc-switch/backups"),
            wsl_home.join(".claude"),
            wsl_home.join(".claude-desktop/configLibrary"),
            wsl_home.join(".agents/skills"),
            desktop_root.join("configLibrary"),
        ] {
            assert!(
                validate_app_config_dir_override_path_with_context(
                    &protected,
                    &host_home,
                    Some(&wsl_home),
                    vec![desktop_root.clone()],
                )
                .is_err(),
                "protected app-data override must be rejected: {}",
                protected.display()
            );
        }

        assert!(validate_app_config_dir_override_path_with_context(
            &wsl_home.join(".cc-switch-cometix"),
            &host_home,
            Some(&wsl_home),
            vec![desktop_root],
        )
        .is_ok());
    }

    #[test]
    fn existing_root_overlaps_not_yet_created_child() {
        let root = tempfile::tempdir().expect("temp root");
        assert!(paths_overlap(
            root.path(),
            &root.path().join("future/child")
        ));
    }

    #[test]
    fn aliased_parent_resolves_not_yet_created_child_into_protected_root() {
        let temp = tempfile::tempdir().expect("temp root");
        let protected = temp.path().join("protected");
        let alias = temp.path().join("alias");
        std::fs::create_dir_all(&protected).expect("create protected root");
        assert!(
            alias_directory(&protected, &alias),
            "create directory alias"
        );

        assert!(paths_overlap(
            &alias.join("future/settings.json"),
            &protected
        ));

        #[cfg(windows)]
        std::fs::remove_dir(&alias).expect("remove test junction");
    }

    #[test]
    fn dangling_directory_alias_still_resolves_into_protected_root() {
        let temp = tempfile::tempdir().expect("temp root");
        let protected = temp.path().join("protected");
        let alias = temp.path().join("alias");
        std::fs::create_dir_all(&protected).expect("create protected root");
        assert!(
            alias_directory(&protected, &alias),
            "create directory alias"
        );

        // Removing the target leaves the symlink/junction itself in place.
        // A future write through that dangling alias must still compare as a
        // protected path instead of being treated as an unrelated missing
        // directory.
        std::fs::remove_dir(&protected).expect("remove protected target");
        assert!(paths_overlap(
            &alias.join("future/settings.json"),
            &protected
        ));

        #[cfg(windows)]
        std::fs::remove_dir(&alias).expect("remove dangling test junction");
    }

    #[test]
    fn private_app_data_rejects_backup_junction_into_upstream_root() {
        let temp = tempfile::tempdir().expect("temp root");
        let private_root = temp.path().join(".cc-switch-cometix");
        let upstream_root = temp.path().join(".cc-switch");
        let backup_alias = private_root.join("backups");
        std::fs::create_dir_all(&private_root).expect("create private root");
        std::fs::create_dir_all(&upstream_root).expect("create upstream root");
        assert!(alias_directory(&upstream_root, &backup_alias));

        assert!(validate_managed_app_data_path(
            &backup_alias,
            &private_root,
            &upstream_root,
            &upstream_root.join("cc-switch.db"),
        )
        .is_err());

        #[cfg(windows)]
        std::fs::remove_dir(&backup_alias).expect("remove test junction");
    }

    #[test]
    fn private_app_database_rejects_hardlink_to_upstream_database() {
        let temp = tempfile::tempdir().expect("temp root");
        let private_root = temp.path().join(".cc-switch-cometix");
        let upstream_root = temp.path().join(".cc-switch");
        let upstream_db = upstream_root.join("cc-switch.db");
        let private_db = private_root.join("cc-switch.db");
        std::fs::create_dir_all(&private_root).expect("create private root");
        std::fs::create_dir_all(&upstream_root).expect("create upstream root");
        std::fs::write(&upstream_db, b"official sentinel").expect("write upstream db");
        std::fs::hard_link(&upstream_db, &private_db).expect("create database hardlink");

        assert!(validate_managed_app_data_path(
            &private_db,
            &private_root,
            &upstream_root,
            &upstream_db,
        )
        .is_err());
    }
}
