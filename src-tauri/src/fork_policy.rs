//! Product-level capability policy for this private fork.
//!
//! Official Claude Code and Claude Desktop are managed by the official CC
//! Switch installation. This fork must never manage their live configuration;
//! it owns only the isolated Claude Code (Cometix) entry. Keep the restriction
//! at the command/startup boundary so legacy database rows remain readable
//! while all externally reachable mutations are denied.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::profile::ProfileScope;
use std::path::{Path, PathBuf};

fn official_claude_management_disabled_error() -> AppError {
    AppError::localized(
        "official_claude_management_disabled",
        "此魔改版不管理官方 Claude Code 或 Claude Desktop，以免与官方 CC Switch 共用并覆盖配置",
        "Official Claude Code and Claude Desktop management is disabled in this private build to avoid conflicting with the official CC Switch installation",
    )
}

fn cometix_directory_conflict_error() -> AppError {
    AppError::localized(
        "settings.claude_cometix_dir.conflict",
        "Claude Code（Cometix）的配置目录与官方 Claude、CC Switch 应用数据或共享 Skills 目录重合，已禁止修改以保护原配置",
        "Claude Code (Cometix) configuration overlaps official Claude, a CC Switch application-data directory, or shared Skills, so mutations are disabled to protect the original configuration",
    )
}

pub(crate) fn ensure_cometix_paths_isolated(
    official: &Path,
    cometix: &Path,
    protected_roots: &[PathBuf],
) -> Result<(), AppError> {
    if crate::app_store::paths_overlap(official, cometix)
        || protected_roots
            .iter()
            .any(|root| crate::app_store::paths_overlap(cometix, root))
    {
        return Err(cometix_directory_conflict_error());
    }

    Ok(())
}

fn ensure_cometix_managed_path_with_roots(
    cometix_root: &Path,
    managed_path: &Path,
    protected_roots: &[PathBuf],
) -> Result<(), AppError> {
    if !crate::app_store::path_is_same_or_nested(managed_path, cometix_root)
        || protected_roots
            .iter()
            .any(|protected| crate::app_store::paths_overlap(managed_path, protected))
    {
        return Err(cometix_directory_conflict_error());
    }

    // Canonical paths reveal symlinks and junctions, but hard links retain the
    // managed lexical path. Claude keeps the same relative layout in both
    // roots, so compare an existing Cometix file with the corresponding file
    // under every protected root as well as with protected file roots such as
    // `~/.claude.json`.
    let managed_path_is_file = match std::fs::metadata(managed_path) {
        Ok(metadata) => metadata.is_file(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err(cometix_directory_conflict_error()),
    };
    if managed_path_is_file {
        if crate::app_store::existing_file_has_multiple_links(managed_path) {
            return Err(cometix_directory_conflict_error());
        }
        let relative = managed_path.strip_prefix(cometix_root).ok();
        let aliases_protected_file = protected_roots.iter().any(|protected| {
            crate::app_store::same_existing_file_identity(managed_path, protected)
                || relative.is_some_and(|relative| {
                    crate::app_store::same_existing_file_identity(
                        managed_path,
                        &protected.join(relative),
                    )
                })
        });
        if aliases_protected_file {
            return Err(cometix_directory_conflict_error());
        }
    }

    Ok(())
}

pub(crate) fn cometix_protected_roots_with_wsl_home(
    home: &Path,
    official: &Path,
    wsl_home: Option<&Path>,
    desktop_roots: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots = vec![
        official.to_path_buf(),
        home.join(".claude"),
        home.join(".claude.json"),
        home.join(".claude-desktop"),
        home.join(".cc-switch"),
        home.join(".agents").join("skills"),
        crate::config::get_app_config_dir(),
    ];
    if let Some(wsl_home) = wsl_home {
        roots.extend([
            wsl_home.join(".claude"),
            wsl_home.join(".claude.json"),
            wsl_home.join(".claude-desktop"),
            wsl_home.join(".cc-switch"),
            wsl_home.join(".agents").join("skills"),
        ]);
    }
    roots.extend(desktop_roots);
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(target_os = "windows")]
fn wsl_probe_error(detail: impl std::fmt::Display) -> AppError {
    log::error!("WSL Cometix isolation probe failed: {detail}");
    AppError::localized(
        "settings.claude_cometix_dir.wsl_probe_failed",
        "无法确认 WSL 中 Claude Code（Cometix）的隔离目录，已拒绝操作以保护官方配置",
        "Unable to verify the WSL Claude Code (Cometix) isolation boundary; the operation was refused to protect official configuration",
    )
}

#[cfg(target_os = "windows")]
fn valid_wsl_distro_name(distro: &str) -> bool {
    !distro.is_empty()
        && distro.len() <= 64
        && distro
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(target_os = "windows")]
pub(crate) fn wsl_distro_from_unc_path(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};

    let Component::Prefix(prefix) = path.components().next()? else {
        return None;
    };
    let (server, share) = match prefix.kind() {
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => (server, share),
        _ => return None,
    };
    let server = server.to_string_lossy();
    if !server.eq_ignore_ascii_case("wsl$") && !server.eq_ignore_ascii_case("wsl.localhost") {
        return None;
    }
    let distro = share.to_string_lossy();
    valid_wsl_distro_name(&distro).then(|| distro.into_owned())
}

#[cfg(target_os = "windows")]
pub(crate) fn wsl_home_unc_path(distro: &str, linux_home: &[u8]) -> Result<PathBuf, AppError> {
    if !valid_wsl_distro_name(distro) {
        return Err(wsl_probe_error("invalid distro name"));
    }
    let linux_home = std::str::from_utf8(linux_home)
        .map_err(|error| wsl_probe_error(format!("HOME is not UTF-8: {error}")))?
        .trim_end_matches(['\r', '\n']);
    if !linux_home.starts_with('/')
        || linux_home == "/"
        || linux_home.contains('\\')
        || linux_home.chars().any(char::is_control)
    {
        return Err(wsl_probe_error("invalid HOME path"));
    }

    let mut unc_home = PathBuf::from(format!(r"\\wsl.localhost\{distro}"));
    for component in linux_home
        .split('/')
        .filter(|component| !component.is_empty())
    {
        if matches!(component, "." | "..") {
            return Err(wsl_probe_error("HOME contains a traversal component"));
        }
        unc_home.push(component);
    }
    Ok(unc_home)
}

#[cfg(target_os = "windows")]
fn resolve_wsl_home_for_distro_with<F>(distro: &str, probe: F) -> Result<PathBuf, AppError>
where
    F: FnOnce(&str) -> Result<Vec<u8>, String>,
{
    if !valid_wsl_distro_name(distro) {
        return Err(wsl_probe_error("invalid distro name"));
    }
    let output = probe(distro).map_err(wsl_probe_error)?;
    wsl_home_unc_path(distro, &output)
}

#[cfg(target_os = "windows")]
pub(crate) fn resolve_wsl_home_for_distro(distro: &str) -> Result<PathBuf, AppError> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    resolve_wsl_home_for_distro_with(distro, |distro| {
        let output = std::process::Command::new("wsl.exe")
            .args(["-d", distro, "--exec", "sh", "-lc", "printf '%s' \"$HOME\""])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "distro HOME probe exited with {:?}",
                output.status.code()
            ));
        }
        Ok(output.stdout)
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn resolve_cometix_wsl_home(path: &Path) -> Result<Option<PathBuf>, AppError> {
    wsl_distro_from_unc_path(path)
        .map(|distro| resolve_wsl_home_for_distro(&distro))
        .transpose()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn resolve_cometix_wsl_home(_path: &Path) -> Result<Option<PathBuf>, AppError> {
    Ok(None)
}

#[cfg(target_os = "windows")]
fn wsl_linux_path_from_unc(path: &Path, expected_distro: &str) -> Result<String, AppError> {
    use std::path::Component;

    let distro = wsl_distro_from_unc_path(path)
        .ok_or_else(|| wsl_probe_error("path is not a valid WSL UNC path"))?;
    if !distro.eq_ignore_ascii_case(expected_distro) {
        return Err(wsl_probe_error("path belongs to another WSL distro"));
    }

    let mut parts = Vec::new();
    for component in path.components().skip(1) {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => {
                let part = part
                    .to_str()
                    .ok_or_else(|| wsl_probe_error("WSL path is not Unicode"))?;
                if part.is_empty()
                    || matches!(part, "." | "..")
                    || part.contains('/')
                    || part.chars().any(char::is_control)
                {
                    return Err(wsl_probe_error("invalid WSL path component"));
                }
                parts.push(part.to_string());
            }
            Component::ParentDir | Component::Prefix(_) => {
                return Err(wsl_probe_error("invalid WSL path traversal"));
            }
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

#[cfg(target_os = "windows")]
fn windows_path_to_wsl_path(distro: &str, path: &Path) -> Result<String, AppError> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    if wsl_distro_from_unc_path(path).is_some() {
        return wsl_linux_path_from_unc(path, distro);
    }
    let raw_path = path
        .to_str()
        .ok_or_else(|| wsl_probe_error("Windows protected path is not Unicode"))?;
    let output = std::process::Command::new("wsl.exe")
        .args(["-d", distro, "--exec", "wslpath", "-a", "-u", raw_path])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(wsl_probe_error)?;
    if !output.status.success() {
        return Err(wsl_probe_error(format!(
            "wslpath exited with {:?}",
            output.status.code()
        )));
    }
    let translated = std::str::from_utf8(&output.stdout)
        .map_err(wsl_probe_error)?
        .trim_end_matches(['\r', '\n']);
    if !translated.starts_with('/') || translated.chars().any(char::is_control) {
        return Err(wsl_probe_error("wslpath returned an invalid path"));
    }
    Ok(translated.to_string())
}

#[cfg(target_os = "windows")]
const WSL_TREE_GUARD_SCRIPT: &str = r#"set -eu
ccs_path_key() {
  printf '%s' "$1" | LC_ALL=C tr 'A-Z' 'a-z'
}
ccs_managed=$1
ccs_require_contained=$2
shift 2
ccs_managed_real=$(realpath -m -- "$ccs_managed")
ccs_managed_key=$(ccs_path_key "$ccs_managed_real")
for ccs_protected do
  ccs_protected_real=$(realpath -m -- "$ccs_protected")
  ccs_protected_key=$(ccs_path_key "$ccs_protected_real")
  case "$ccs_managed_key/" in "$ccs_protected_key/"*) exit 71 ;; esac
  case "$ccs_protected_key/" in "$ccs_managed_key/"*) exit 71 ;; esac
done
if [ ! -e "$ccs_managed" ] && [ ! -L "$ccs_managed" ]; then
  exit 0
fi
ccs_failure=$(mktemp)
trap 'rm -f -- "$ccs_failure"' EXIT HUP INT TERM
find "$ccs_managed" -exec sh -c '
  ccs_path_key() {
    printf "%s" "$1" | LC_ALL=C tr "A-Z" "a-z"
  }
  ccs_entry=$1
  ccs_failure=$2
  ccs_managed_real=$3
  ccs_require_contained=$4
  shift 4
  ccs_reject() { printf x >> "$ccs_failure"; exit 0; }
  ccs_entry_real=$(realpath -m -- "$ccs_entry") || ccs_reject
  ccs_entry_key=$(ccs_path_key "$ccs_entry_real") || ccs_reject
  ccs_managed_key=$(ccs_path_key "$ccs_managed_real") || ccs_reject
  if [ "$ccs_require_contained" = 1 ]; then
    case "$ccs_entry_key/" in "$ccs_managed_key/"*) ;; *) ccs_reject ;; esac
  fi
  for ccs_protected do
    ccs_protected_real=$(realpath -m -- "$ccs_protected") || ccs_reject
    ccs_protected_key=$(ccs_path_key "$ccs_protected_real") || ccs_reject
    case "$ccs_entry_key/" in "$ccs_protected_key/"*) ccs_reject ;; esac
    case "$ccs_protected_key/" in "$ccs_entry_key/"*) ccs_reject ;; esac
  done
  if [ -f "$ccs_entry" ] && [ ! -L "$ccs_entry" ]; then
    ccs_links=$(stat -c %h -- "$ccs_entry") || ccs_reject
    [ "$ccs_links" -le 1 ] || ccs_reject
  fi
' cc-switch-wsl-entry {} "$ccs_failure" "$ccs_managed_real" "$ccs_require_contained" "$@" \;
[ ! -s "$ccs_failure" ]
"#;

#[cfg(target_os = "windows")]
pub(crate) fn ensure_wsl_tree_isolated(
    distro: &str,
    managed_root: &Path,
    protected_roots: &[PathBuf],
    require_contained: bool,
) -> Result<(), AppError> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let managed = wsl_linux_path_from_unc(managed_root, distro)?;
    let mut protected = protected_roots
        .iter()
        .map(|path| windows_path_to_wsl_path(distro, path))
        .collect::<Result<Vec<_>, _>>()?;
    protected.sort();
    protected.dedup();

    let mut command = std::process::Command::new("wsl.exe");
    command.args([
        "-d",
        distro,
        "--exec",
        "sh",
        "-c",
        WSL_TREE_GUARD_SCRIPT,
        "cc-switch-wsl-guard",
        &managed,
        if require_contained { "1" } else { "0" },
    ]);
    command.args(&protected).creation_flags(CREATE_NO_WINDOW);
    let output = command.output().map_err(wsl_probe_error)?;
    if !output.status.success() {
        return Err(wsl_probe_error(format!(
            "in-distro tree guard exited with {:?}",
            output.status.code()
        )));
    }
    Ok(())
}

fn desktop_protected_roots(wsl: bool) -> Result<Vec<PathBuf>, AppError> {
    match crate::claude_desktop_config::get_protected_config_roots() {
        Ok(roots) => Ok(roots),
        Err(error) if wsl => Err(error),
        Err(_) => Ok(Vec::new()),
    }
}

pub(crate) fn ensure_cometix_config_paths_isolated(
    official: &Path,
    cometix: &Path,
) -> Result<(), AppError> {
    let home = crate::config::get_home_dir();
    let wsl_home = resolve_cometix_wsl_home(cometix)?;
    let desktop_roots = desktop_protected_roots(wsl_home.is_some())?;
    let protected_roots =
        cometix_protected_roots_with_wsl_home(&home, official, wsl_home.as_deref(), desktop_roots);
    ensure_cometix_paths_isolated(official, cometix, &protected_roots)?;

    #[cfg(target_os = "windows")]
    if let Some(distro) = wsl_distro_from_unc_path(cometix) {
        ensure_wsl_tree_isolated(&distro, cometix, &protected_roots, true)?;
    }
    Ok(())
}

pub(crate) fn ensure_cometix_config_dir_isolated() -> Result<(), AppError> {
    ensure_cometix_config_paths_isolated(
        &crate::config::get_claude_config_dir(),
        &crate::config::get_claude_cometix_config_dir(),
    )
}

pub(crate) fn ensure_cometix_managed_config_path_isolated(
    managed_path: &Path,
) -> Result<(), AppError> {
    ensure_cometix_config_dir_isolated()?;
    let home = crate::config::get_home_dir();
    let official = crate::config::get_claude_config_dir();
    let cometix = crate::config::get_claude_cometix_config_dir();
    let wsl_home = resolve_cometix_wsl_home(&cometix)?;
    let protected_roots = cometix_protected_roots_with_wsl_home(
        &home,
        &official,
        wsl_home.as_deref(),
        desktop_protected_roots(wsl_home.is_some())?,
    );
    ensure_cometix_managed_path_with_roots(&cometix, managed_path, &protected_roots)
}

/// Validate every existing entry in the Cometix configuration tree before an
/// external `hlclaude` process is allowed to run. The CLI can write far more
/// than the handful of files CC Switch projects itself (projects, sessions,
/// plugins, caches, history, and future version-specific paths), so checking
/// only the root or a fixed list of children would leave junction/symlink
/// aliases as an escape hatch into the official Claude tree.
pub(crate) fn ensure_cometix_config_tree_isolated() -> Result<(), AppError> {
    ensure_cometix_config_dir_isolated()?;
    let root = crate::config::get_claude_cometix_config_dir();
    if !root.exists() {
        return Ok(());
    }

    let home = crate::config::get_home_dir();
    let official = crate::config::get_claude_config_dir();
    let wsl_home = resolve_cometix_wsl_home(&root)?;
    let protected_roots = cometix_protected_roots_with_wsl_home(
        &home,
        &official,
        wsl_home.as_deref(),
        desktop_protected_roots(wsl_home.is_some())?,
    );
    ensure_cometix_tree_with_roots(&root, &root, &protected_roots)
}

pub(crate) fn ensure_cometix_tree_with_roots(
    cometix_root: &Path,
    directory: &Path,
    protected_roots: &[PathBuf],
) -> Result<(), AppError> {
    let mut visited = std::collections::HashSet::new();
    ensure_cometix_tree_with_roots_inner(cometix_root, directory, protected_roots, &mut visited)
}

fn ensure_cometix_tree_with_roots_inner(
    cometix_root: &Path,
    directory: &Path,
    protected_roots: &[PathBuf],
    visited: &mut std::collections::HashSet<PathBuf>,
) -> Result<(), AppError> {
    ensure_cometix_managed_path_with_roots(cometix_root, directory, protected_roots)?;
    let directory_identity = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.into());
    if !visited.insert(directory_identity) {
        return Ok(());
    }

    let entries = std::fs::read_dir(directory).map_err(|error| AppError::io(directory, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| AppError::io(directory, error))?;
        let path = entry.path();
        ensure_cometix_managed_path_with_roots(cometix_root, &path, protected_roots)?;

        // `symlink_metadata` avoids following a link after it has been
        // validated. Windows junctions that report as directories are still
        // rejected above when canonicalization escapes the Cometix root.
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|error| AppError::io(&path, error))?;
        if metadata.file_type().is_dir() {
            ensure_cometix_tree_with_roots_inner(cometix_root, &path, protected_roots, visited)?;
        }
    }

    Ok(())
}

pub(crate) fn app_management_allowed(app: &AppType) -> bool {
    match app {
        AppType::Claude | AppType::ClaudeDesktop => false,
        AppType::ClaudeCometix => ensure_cometix_config_dir_isolated().is_ok(),
        _ => true,
    }
}

pub(crate) fn ensure_app_management_allowed(app: &AppType) -> Result<(), AppError> {
    match app {
        AppType::Claude | AppType::ClaudeDesktop => {
            Err(official_claude_management_disabled_error())
        }
        AppType::ClaudeCometix => ensure_cometix_config_dir_isolated(),
        _ => Ok(()),
    }
}

pub(crate) fn profile_scope_management_allowed(scope: ProfileScope) -> bool {
    // ProfileScope has no Cometix variant: Claude is the official ~/.claude
    // scope, while Cometix is managed through its own AppType commands.
    !matches!(scope, ProfileScope::Claude | ProfileScope::ClaudeDesktop)
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

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_cometix_home_resolution_fails_closed_when_the_distro_probe_fails() {
        let result = resolve_wsl_home_for_distro_with("Ubuntu-24.04", |_| {
            Err("distro unavailable".to_string())
        });

        assert!(result.is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_cometix_paths_use_the_selected_distros_real_home() {
        let config = Path::new(r"\\wsl$\Ubuntu-24.04\srv\profiles\cometix");
        assert_eq!(
            wsl_distro_from_unc_path(config).as_deref(),
            Some("Ubuntu-24.04")
        );
        assert_eq!(
            wsl_home_unc_path("Ubuntu-24.04", b"/home/alice\n").expect("valid WSL HOME"),
            PathBuf::from(r"\\wsl.localhost\Ubuntu-24.04\home\alice")
        );
        assert!(wsl_home_unc_path("Ubuntu", b"/home/../root").is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_guard_rejects_a_drvfs_case_variant_of_a_windows_protected_root() {
        if resolve_wsl_home_for_distro("Ubuntu").is_err() {
            // WSL is optional in CI. The pure parsing/probe-failure tests above
            // still run everywhere; this exercises the real distro adapter
            // when the development host provides the Ubuntu fixture.
            return;
        }
        let temp = tempfile::tempdir().expect("Windows protected temp root");
        let translated =
            windows_path_to_wsl_path("Ubuntu", temp.path()).expect("translate Windows temp root");
        if !translated
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("/mnt/"))
        {
            return;
        }
        let managed_unc = PathBuf::from(format!(
            r"\\wsl.localhost\Ubuntu{}",
            translated.to_ascii_lowercase().replace('/', r"\")
        ));

        assert!(ensure_wsl_tree_isolated(
            "Ubuntu",
            &managed_unc,
            &[temp.path().to_path_buf()],
            false,
        )
        .is_err());
    }

    #[test]
    fn cometix_path_validation_protects_the_selected_wsl_home() {
        let temp = tempfile::tempdir().expect("temp dir");
        let host_home = temp.path().join("host");
        let wsl_home = temp.path().join("wsl-home");
        let official = host_home.join(".claude");
        let protected = cometix_protected_roots_with_wsl_home(
            &host_home,
            &official,
            Some(&wsl_home),
            Vec::new(),
        );

        assert!(ensure_cometix_paths_isolated(
            &official,
            &wsl_home.join(".claude/projects"),
            &protected,
        )
        .is_err());
        assert!(ensure_cometix_paths_isolated(
            &official,
            &wsl_home.join(".claude.json"),
            &protected,
        )
        .is_err());
        assert!(ensure_cometix_paths_isolated(
            &official,
            &wsl_home.join(".cc-switch/backups"),
            &protected,
        )
        .is_err());
        assert!(ensure_cometix_paths_isolated(
            &official,
            &wsl_home.join(".claude-desktop/configLibrary"),
            &protected,
        )
        .is_err());
        assert!(ensure_cometix_paths_isolated(
            &official,
            &wsl_home.join(".agents/skills/private-skill"),
            &protected,
        )
        .is_err());
        assert!(
            ensure_cometix_paths_isolated(&official, &wsl_home.join(".agents"), &protected,)
                .is_err()
        );
        assert!(
            ensure_cometix_paths_isolated(&official, &wsl_home.join(".hlclaude"), &protected,)
                .is_ok()
        );
    }

    #[cfg(unix)]
    fn symlink_dir(source: &std::path::Path, destination: &std::path::Path) -> bool {
        std::os::unix::fs::symlink(source, destination).is_ok()
    }

    #[cfg(windows)]
    fn symlink_dir(source: &std::path::Path, destination: &std::path::Path) -> bool {
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
    fn private_fork_rejects_official_claude_apps_but_allows_cometix() {
        assert!(ensure_app_management_allowed(&AppType::ClaudeDesktop).is_err());
        assert!(ensure_app_management_allowed(&AppType::Claude).is_err());
        assert!(ensure_app_management_allowed(&AppType::ClaudeCometix).is_ok());
        assert!(ensure_app_management_allowed(&AppType::Codex).is_ok());
    }

    #[test]
    fn private_fork_rejects_official_claude_profile_scopes() {
        assert!(ensure_profile_scope_management_allowed(ProfileScope::ClaudeDesktop).is_err());
        assert!(ensure_profile_scope_management_allowed(ProfileScope::Claude).is_err());
        assert!(ensure_profile_scope_management_allowed(ProfileScope::Codex).is_ok());
    }

    #[test]
    fn cometix_runtime_paths_reject_official_and_app_data_overlap() {
        let temp = tempfile::tempdir().expect("temp dir");
        let official = temp.path().join(".claude");
        let cometix = temp.path().join(".hlclaude");
        let upstream_root = temp.path().join(".cc-switch");
        let fork_root = temp.path().join(".cc-switch-cometix");
        let protected_roots = [upstream_root.clone(), fork_root.clone()];

        assert!(ensure_cometix_paths_isolated(&official, &official, &protected_roots).is_err());
        assert!(
            ensure_cometix_paths_isolated(&official, &upstream_root, &protected_roots).is_err()
        );
        assert!(ensure_cometix_paths_isolated(&official, &fork_root, &protected_roots).is_err());
        assert!(ensure_cometix_paths_isolated(&official, &cometix, &protected_roots).is_ok());
    }

    #[test]
    fn cometix_runtime_paths_reject_existing_directory_alias_to_official() {
        let temp = tempfile::tempdir().expect("temp dir");
        let official = temp.path().join(".claude");
        let cometix_alias = temp.path().join(".hlclaude");
        std::fs::create_dir_all(&official).expect("create official dir");

        // Windows without Developer Mode may not permit creating a symlink.
        // The exact-overlap assertion above remains the portable regression;
        // where aliases are available this additionally proves canonical-path
        // detection for a real junction/symlink-style setup.
        if !symlink_dir(&official, &cometix_alias) {
            return;
        }

        assert!(ensure_cometix_paths_isolated(&official, &cometix_alias, &[]).is_err());

        #[cfg(windows)]
        std::fs::remove_dir(&cometix_alias).expect("remove test junction");
    }

    #[test]
    fn cometix_managed_subpaths_reject_an_inner_alias_outside_the_cometix_root() {
        let temp = tempfile::tempdir().expect("temp dir");
        let official = temp.path().join(".claude");
        let cometix = temp.path().join(".hlclaude");
        let official_projects = official.join("projects");
        let cometix_projects = cometix.join("projects");
        std::fs::create_dir_all(&official_projects).expect("create official projects");
        std::fs::create_dir_all(&cometix).expect("create Cometix root");
        assert!(
            symlink_dir(&official_projects, &cometix_projects),
            "create inner directory alias"
        );

        assert!(ensure_cometix_managed_path_with_roots(
            &cometix,
            &cometix_projects,
            std::slice::from_ref(&official),
        )
        .is_err());
        assert!(ensure_cometix_managed_path_with_roots(
            &cometix,
            &cometix.join("settings.json"),
            std::slice::from_ref(&official),
        )
        .is_ok());

        #[cfg(windows)]
        std::fs::remove_dir(&cometix_projects).expect("remove test junction");
    }

    #[test]
    fn cometix_tree_rejects_a_nested_directory_alias_to_official_data() {
        let temp = tempfile::tempdir().expect("temp dir");
        let official = temp.path().join(".claude");
        let cometix = temp.path().join(".hlclaude");
        let official_sessions = official.join("projects");
        let cometix_sessions = cometix.join("projects");
        std::fs::create_dir_all(&official_sessions).expect("create official sessions");
        std::fs::create_dir_all(&cometix).expect("create Cometix root");
        assert!(
            symlink_dir(&official_sessions, &cometix_sessions),
            "create nested alias"
        );

        assert!(ensure_cometix_tree_with_roots(
            &cometix,
            &cometix,
            std::slice::from_ref(&official),
        )
        .is_err());

        #[cfg(windows)]
        std::fs::remove_dir(&cometix_sessions).expect("remove test junction");
    }

    #[test]
    fn cometix_managed_file_rejects_a_hard_link_to_the_official_relative_file() {
        let temp = tempfile::tempdir().expect("temp dir");
        let official = temp.path().join(".claude");
        let cometix = temp.path().join(".hlclaude");
        let official_settings = official.join("settings.json");
        let cometix_settings = cometix.join("settings.json");
        std::fs::create_dir_all(&official).expect("create official root");
        std::fs::create_dir_all(&cometix).expect("create Cometix root");
        std::fs::write(&official_settings, "official sentinel").expect("seed official file");
        std::fs::hard_link(&official_settings, &cometix_settings).expect("create hard link");

        assert!(ensure_cometix_managed_path_with_roots(
            &cometix,
            &cometix_settings,
            std::slice::from_ref(&official),
        )
        .is_err());
    }

    #[test]
    fn cometix_managed_file_rejects_a_cross_name_hard_link_to_official_data() {
        let temp = tempfile::tempdir().expect("temp dir");
        let official = temp.path().join(".claude");
        let cometix = temp.path().join(".hlclaude");
        let official_prompt = official.join("CLAUDE.md");
        let cometix_settings = cometix.join("settings.json");
        std::fs::create_dir_all(&official).expect("create official root");
        std::fs::create_dir_all(&cometix).expect("create Cometix root");
        std::fs::write(&official_prompt, "official sentinel").expect("seed official file");
        std::fs::hard_link(&official_prompt, &cometix_settings).expect("create hard link");

        assert!(ensure_cometix_managed_path_with_roots(
            &cometix,
            &cometix_settings,
            std::slice::from_ref(&official),
        )
        .is_err());
    }
}
