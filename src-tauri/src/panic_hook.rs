//! Panic Hook 模块
//!
//! 在应用崩溃时捕获 panic 信息并记录到 `<app_config_dir>/crash.log` 文件中
//! （默认 `~/.cc-switch-cometix/crash.log`）。
//! 便于用户和开发者诊断闪退问题。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 应用版本号（从 Cargo.toml 读取）
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const CRASH_LOG_MAX_SIZE: u64 = 5 * 1024 * 1024;
const CRASH_LOG_ARCHIVES_TO_KEEP: usize = 2;

static APP_CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
static CRASH_LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn init_app_config_dir(dir: PathBuf) {
    let _ = APP_CONFIG_DIR.set(dir);
}

/// 获取默认应用配置目录（不会 panic）
fn default_app_config_dir() -> PathBuf {
    crate::config::get_default_app_config_dir()
}

/// 获取应用配置目录（优先使用初始化时写入的值；不会 panic）
fn get_app_config_dir() -> PathBuf {
    APP_CONFIG_DIR
        .get()
        .cloned()
        .unwrap_or_else(default_app_config_dir)
}

/// 获取崩溃日志文件路径
fn get_crash_log_path() -> PathBuf {
    get_app_config_dir().join("crash.log")
}

fn rotated_crash_log_path(path: &Path, index: usize) -> PathBuf {
    let mut rotated = path.as_os_str().to_os_string();
    rotated.push(format!(".{index}"));
    PathBuf::from(rotated)
}

fn private_path_guard(path: &Path) -> std::io::Result<()> {
    crate::app_store::ensure_private_app_data_path_isolated(path).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("拒绝不安全的崩溃日志路径 {}: {error}", path.display()),
        )
    })
}

fn guard_crash_log_family(path: &Path, archives_to_keep: usize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        private_path_guard(parent)?;
    }
    private_path_guard(path)?;
    for index in 1..=archives_to_keep {
        private_path_guard(&rotated_crash_log_path(path, index))?;
    }
    Ok(())
}

fn prepare_crash_log_parent(path: &Path) -> std::io::Result<()> {
    guard_crash_log_family(path, CRASH_LOG_ARCHIVES_TO_KEEP)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        // Re-resolve after creation: a pre-existing parent component may be a
        // symlink/junction, or the directory may have changed concurrently.
        private_path_guard(parent)?;
    }
    guard_crash_log_family(path, CRASH_LOG_ARCHIVES_TO_KEEP)
}

fn rotate_crash_log_if_needed_with_limit(
    path: &Path,
    max_size: u64,
    archives_to_keep: usize,
) -> std::io::Result<()> {
    guard_crash_log_family(path, archives_to_keep)?;
    private_path_guard(path)?;
    let size = match fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if size < max_size || archives_to_keep == 0 {
        return Ok(());
    }

    for index in (1..=archives_to_keep).rev() {
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            rotated_crash_log_path(path, index - 1)
        };
        private_path_guard(&source)?;
        if !source.exists() {
            continue;
        }

        let destination = rotated_crash_log_path(path, index);
        private_path_guard(&destination)?;
        if destination.exists() {
            private_path_guard(&destination)?;
            fs::remove_file(&destination)?;
        }
        private_path_guard(&source)?;
        private_path_guard(&destination)?;
        fs::rename(source, destination)?;
    }

    Ok(())
}

fn rotate_crash_log_if_needed(path: &Path) -> std::io::Result<()> {
    rotate_crash_log_if_needed_with_limit(path, CRASH_LOG_MAX_SIZE, CRASH_LOG_ARCHIVES_TO_KEEP)
}

/// 获取日志目录路径
pub fn get_log_dir() -> PathBuf {
    get_app_config_dir().join("logs")
}

/// 安全获取环境信息（不会 panic）
fn get_system_info() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let family = std::env::consts::FAMILY;

    // 安全获取当前工作目录
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // 安全获取当前线程信息
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    let thread_id = format!("{:?}", thread.id());

    format!(
        "OS: {os} ({family})\n\
         Arch: {arch}\n\
         App Version: {APP_VERSION}\n\
         Working Dir: {cwd}\n\
         Thread: {thread_name} (ID: {thread_id})"
    )
}

/// 设置 panic hook，捕获崩溃信息并写入日志文件
///
/// 在应用启动时调用此函数，确保任何 panic 都会被记录。
/// 日志格式包含：
/// - 时间戳
/// - 应用版本和系统信息
/// - Panic 信息
/// - 发生位置（文件:行号）
/// - Backtrace（完整调用栈）
pub fn setup_panic_hook() {
    // 启用 backtrace（确保 release 模式也能捕获）
    if std::env::var("RUST_BACKTRACE").is_err() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }

    let default_hook = panic::take_hook();

    panic::set_hook(Box::new(move |panic_info| {
        let log_path = get_crash_log_path();

        // Guard both the directory and every final/rotated file before any
        // filesystem mutation. Failure only disables file persistence; the
        // report is still emitted to stderr and the default hook still runs.
        let persistence_ready = prepare_crash_log_parent(&log_path).is_ok();

        // 构建崩溃信息（使用 catch_unwind 保护时间格式化，避免嵌套 panic）
        let timestamp = std::panic::catch_unwind(|| {
            chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()
        })
        .unwrap_or_else(|_| {
            // chrono panic 时回退到 unix timestamp
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| format!("unix:{}.{:03}", d.as_secs(), d.subsec_millis()))
                .unwrap_or_else(|_| "unknown".to_string())
        });

        // 获取系统信息
        let system_info = std::panic::catch_unwind(get_system_info)
            .unwrap_or_else(|_| "Failed to get system info".to_string());

        // 获取 panic 消息（尝试多种方式提取）
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            // 尝试使用 Display trait
            format!("{panic_info}")
        };

        // 获取位置信息
        let location = if let Some(loc) = panic_info.location() {
            format!(
                "File: {}\n         Line: {}\n         Column: {}",
                loc.file(),
                loc.line(),
                loc.column()
            )
        } else {
            "Unknown location".to_string()
        };

        // 捕获 backtrace（完整调用栈）
        let backtrace = std::backtrace::Backtrace::force_capture();
        let backtrace_str = format!("{backtrace}");

        // 格式化日志条目
        let separator = "=".repeat(80);
        let sub_separator = "-".repeat(40);
        let crash_entry = format!(
            r#"
{separator}
[CRASH REPORT] {timestamp}
{separator}

{sub_separator}
System Information
{sub_separator}
{system_info}

{sub_separator}
Error Details
{sub_separator}
Message: {message}

Location: {location}

{sub_separator}
Stack Trace (Backtrace)
{sub_separator}
{backtrace_str}

{separator}
"#
        );

        // 将 size check、轮转和追加合成同一个临界区，避免多线程同时 panic
        // 时两个 hook 竞争 rename 而丢失归档。
        let crash_log_guard = CRASH_LOG_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let saved = if persistence_ready
            && rotate_crash_log_if_needed(&log_path).is_ok()
            && private_path_guard(&log_path).is_ok()
        {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
                // Recheck immediately before each write. This catches a
                // symlink/junction or hard-link swap between open and append.
                if private_path_guard(&log_path).is_ok() {
                    let wrote = file.write_all(crash_entry.as_bytes()).is_ok();
                    let flushed = file.flush().is_ok();
                    wrote && flushed
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        drop(crash_log_guard);

        if saved {
            eprintln!("\n[CC-Switch] Crash log saved to: {}", log_path.display());
        }

        // 同时输出到 stderr（便于开发调试）
        eprintln!("{crash_entry}");

        // 调用默认 hook
        default_hook(panic_info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHomeGuard(Option<std::ffi::OsString>);

    impl TestHomeGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", path);
            Self(previous)
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => std::env::set_var("CC_SWITCH_TEST_HOME", previous),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
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

    #[test]
    fn test_crash_log_path() {
        let path = get_crash_log_path();
        assert!(path.ends_with("crash.log"));
        assert!(path.to_string_lossy().contains(".cc-switch-cometix"));
    }

    #[test]
    fn test_system_info() {
        let info = get_system_info();
        assert!(info.contains("OS:"));
        assert!(info.contains("Arch:"));
        assert!(info.contains("App Version:"));
    }

    #[test]
    #[serial_test::serial]
    fn crash_log_rotation_keeps_bounded_archives() {
        let home = tempfile::tempdir().unwrap();
        let _home_guard = TestHomeGuard::set(home.path());
        let dir = home.path().join(".cc-switch-cometix");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crash.log");

        fs::write(&path, b"first").unwrap();
        rotate_crash_log_if_needed_with_limit(&path, 4, 2).unwrap();
        assert!(!path.exists());
        assert_eq!(
            fs::read(rotated_crash_log_path(&path, 1)).unwrap(),
            b"first"
        );

        fs::write(&path, b"second").unwrap();
        rotate_crash_log_if_needed_with_limit(&path, 4, 2).unwrap();
        assert_eq!(
            fs::read(rotated_crash_log_path(&path, 1)).unwrap(),
            b"second"
        );
        assert_eq!(
            fs::read(rotated_crash_log_path(&path, 2)).unwrap(),
            b"first"
        );

        fs::write(&path, b"third").unwrap();
        rotate_crash_log_if_needed_with_limit(&path, 4, 2).unwrap();
        assert_eq!(
            fs::read(rotated_crash_log_path(&path, 1)).unwrap(),
            b"third"
        );
        assert_eq!(
            fs::read(rotated_crash_log_path(&path, 2)).unwrap(),
            b"second"
        );
        assert!(!rotated_crash_log_path(&path, 3).exists());
    }

    #[test]
    #[serial_test::serial]
    fn crash_log_rotation_rejects_private_root_alias_to_upstream_data() {
        let home = tempfile::tempdir().expect("temp home");
        let _home_guard = TestHomeGuard::set(home.path());
        let upstream_root = home.path().join(".cc-switch");
        let private_alias = home.path().join(".cc-switch-cometix");
        std::fs::create_dir_all(&upstream_root).expect("create upstream root");
        assert!(
            alias_directory(&upstream_root, &private_alias),
            "create private-root alias"
        );

        let upstream_log = upstream_root.join("crash.log");
        std::fs::write(&upstream_log, b"official crash sentinel").expect("write upstream sentinel");

        let result = rotate_crash_log_if_needed_with_limit(&private_alias.join("crash.log"), 1, 2);
        let sentinel_untouched =
            std::fs::read(&upstream_log).ok() == Some(b"official crash sentinel".to_vec());
        let archive_created = upstream_root.join("crash.log.1").exists();

        #[cfg(windows)]
        std::fs::remove_dir(&private_alias).expect("remove test junction");

        assert!(
            result.is_err(),
            "unsafe crash-log rotation must be rejected"
        );
        assert!(
            sentinel_untouched,
            "upstream crash log must remain untouched"
        );
        assert!(!archive_created, "upstream archive must not be created");
    }

    #[test]
    #[serial_test::serial]
    fn crash_log_rotation_rejects_active_log_hardlink_to_upstream_database() {
        let home = tempfile::tempdir().expect("temp home");
        let _home_guard = TestHomeGuard::set(home.path());
        let upstream_root = home.path().join(".cc-switch");
        let private_root = home.path().join(".cc-switch-cometix");
        std::fs::create_dir_all(&upstream_root).expect("create upstream root");
        std::fs::create_dir_all(&private_root).expect("create private root");
        let upstream_db = upstream_root.join("cc-switch.db");
        let crash_log = private_root.join("crash.log");
        std::fs::write(&upstream_db, b"official database sentinel")
            .expect("write upstream database");
        std::fs::hard_link(&upstream_db, &crash_log).expect("create crash-log hardlink");

        let result = rotate_crash_log_if_needed_with_limit(&crash_log, 1, 2);

        assert!(result.is_err(), "unsafe crash-log target must be rejected");
        assert_eq!(
            std::fs::read(&upstream_db).expect("read upstream database"),
            b"official database sentinel"
        );
        assert!(!private_root.join("crash.log.1").exists());
    }
}
