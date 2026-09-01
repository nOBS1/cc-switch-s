use super::env_checker::EnvConflict;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
#[cfg(not(target_os = "windows"))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub backup_path: String,
    pub timestamp: String,
    pub conflicts: Vec<EnvConflict>,
}

fn ensure_conflicts_do_not_target_official_claude(conflicts: &[EnvConflict]) -> Result<(), String> {
    if conflicts.iter().any(|conflict| {
        let name = conflict.var_name.trim().to_ascii_uppercase();
        name.starts_with("ANTHROPIC") || name.starts_with("CLAUDE") || name == "DISABLE_AUTOUPDATER"
    }) {
        return crate::fork_policy::ensure_app_management_allowed(
            &crate::app_config::AppType::Claude,
        )
        .map_err(|error| error.to_string());
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn source_file_path(source_path: &str) -> Result<PathBuf, String> {
    let (path, line) = source_path
        .rsplit_once(':')
        .ok_or_else(|| "无效的文件路径格式".to_string())?;
    if path.trim().is_empty() || line.parse::<usize>().is_err() {
        return Err("无效的文件路径格式".to_string());
    }
    Ok(PathBuf::from(path))
}

#[cfg(not(target_os = "windows"))]
fn is_exact_path(left: &Path, right: &Path) -> bool {
    crate::app_store::path_is_same_or_nested(left, right)
        && crate::app_store::path_is_same_or_nested(right, left)
}

#[cfg(not(target_os = "windows"))]
fn ensure_shell_profile_path_allowed(path: &Path) -> Result<(), String> {
    let home = crate::config::get_home_dir();
    let allowed = [
        home.join(".bashrc"),
        home.join(".bash_profile"),
        home.join(".zshrc"),
        home.join(".zprofile"),
        home.join(".profile"),
        PathBuf::from("/etc/profile"),
        PathBuf::from("/etc/bashrc"),
    ];
    if !allowed.iter().any(|allowed| is_exact_path(path, allowed)) {
        return Err(format!(
            "拒绝修改不受支持的环境配置文件: {}",
            path.display()
        ));
    }

    let mut protected = vec![
        crate::config::get_claude_config_dir(),
        home.join(".claude"),
        home.join(".claude.json"),
        home.join(".claude-desktop"),
        home.join(".cc-switch"),
        crate::config::get_app_config_dir(),
    ];
    if let Ok(desktop_roots) = crate::claude_desktop_config::get_protected_config_roots() {
        protected.extend(desktop_roots);
    }
    if protected
        .iter()
        .any(|root| crate::app_store::paths_overlap(path, root))
    {
        return Err(format!(
            "环境配置文件指向官方 Claude 或 CC Switch 数据，已拒绝修改: {}",
            path.display()
        ));
    }
    Ok(())
}

fn ensure_conflict_sources_are_safe(conflicts: &[EnvConflict]) -> Result<(), String> {
    for conflict in conflicts {
        match conflict.source_type.as_str() {
            "system" => {
                #[cfg(target_os = "windows")]
                if !matches!(
                    conflict.source_path.as_str(),
                    "HKEY_CURRENT_USER\\Environment"
                        | "HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment"
                ) {
                    return Err("不受支持的系统环境变量来源".to_string());
                }
                #[cfg(not(target_os = "windows"))]
                if conflict.source_path != "Process Environment" {
                    return Err("不受支持的系统环境变量来源".to_string());
                }
            }
            "file" => {
                #[cfg(target_os = "windows")]
                return Err("Windows 系统不应该有文件类型的环境变量".to_string());
                #[cfg(not(target_os = "windows"))]
                ensure_shell_profile_path_allowed(&source_file_path(&conflict.source_path)?)?;
            }
            _ => return Err(format!("未知的环境变量来源类型: {}", conflict.source_type)),
        }
    }
    Ok(())
}

/// Delete environment variables with automatic backup
pub fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, String> {
    ensure_conflicts_do_not_target_official_claude(&conflicts)?;
    ensure_conflict_sources_are_safe(&conflicts)?;

    // Step 1: Create backup
    let backup_info = create_backup(&conflicts)?;

    // Step 2: Delete variables
    for conflict in &conflicts {
        match delete_single_env(conflict) {
            Ok(_) => {}
            Err(e) => {
                // If deletion fails, we keep the backup but return error
                return Err(format!(
                    "删除环境变量失败: {}. 备份已保存到: {}",
                    e, backup_info.backup_path
                ));
            }
        }
    }

    Ok(backup_info)
}

/// Create backup file before deletion
fn create_backup(conflicts: &[EnvConflict]) -> Result<BackupInfo, String> {
    // Get backup directory
    let backup_dir = get_backup_dir()?;
    fs::create_dir_all(&backup_dir).map_err(|e| format!("创建备份目录失败: {e}"))?;

    // Generate backup file name with timestamp
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_file = backup_dir.join(format!("env-backup-{timestamp}.json"));
    crate::app_store::ensure_private_app_data_path_isolated(&backup_file)
        .map_err(|error| error.to_string())?;

    // Create backup data
    let backup_info = BackupInfo {
        backup_path: backup_file.to_string_lossy().to_string(),
        timestamp: timestamp.clone(),
        conflicts: conflicts.to_vec(),
    };

    // Write backup file
    let json = serde_json::to_string_pretty(&backup_info)
        .map_err(|e| format!("序列化备份数据失败: {e}"))?;

    fs::write(&backup_file, json).map_err(|e| format!("写入备份文件失败: {e}"))?;

    Ok(backup_info)
}

/// Get backup directory path
fn get_backup_dir() -> Result<PathBuf, String> {
    let path = crate::config::get_app_config_dir().join("backups");
    crate::app_store::ensure_private_app_data_path_isolated(&path)
        .map_err(|error| error.to_string())?;
    Ok(path)
}

/// Delete a single environment variable
#[cfg(target_os = "windows")]
fn delete_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "system" => {
            if conflict.source_path.contains("HKEY_CURRENT_USER") {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER)
                    .open_subkey_with_flags("Environment", KEY_ALL_ACCESS)
                    .map_err(|e| format!("打开注册表失败: {}", e))?;

                hkcu.delete_value(&conflict.var_name)
                    .map_err(|e| format!("删除注册表项失败: {}", e))?;
            } else if conflict.source_path.contains("HKEY_LOCAL_MACHINE") {
                let hklm = RegKey::predef(HKEY_LOCAL_MACHINE)
                    .open_subkey_with_flags(
                        "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                        KEY_ALL_ACCESS,
                    )
                    .map_err(|e| format!("打开系统注册表失败 (需要管理员权限): {}", e))?;

                hklm.delete_value(&conflict.var_name)
                    .map_err(|e| format!("删除系统注册表项失败: {}", e))?;
            }
            Ok(())
        }
        "file" => Err("Windows 系统不应该有文件类型的环境变量".to_string()),
        _ => Err(format!("未知的环境变量来源类型: {}", conflict.source_type)),
    }
}

#[cfg(not(target_os = "windows"))]
fn delete_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "file" => {
            // Parse file path and line number from source_path (format: "path:line")
            let file_path = source_file_path(&conflict.source_path)?;
            ensure_shell_profile_path_allowed(&file_path)?;

            // Read file content
            let content = fs::read_to_string(&file_path)
                .map_err(|e| format!("读取文件失败 {}: {e}", file_path.display()))?;

            // Filter out the line containing the environment variable
            let new_content: Vec<String> = content
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    let export_line = trimmed.strip_prefix("export ").unwrap_or(trimmed);

                    // Check if this line sets the target variable
                    if let Some(eq_pos) = export_line.find('=') {
                        let var_name = export_line[..eq_pos].trim();
                        var_name != conflict.var_name
                    } else {
                        true
                    }
                })
                .map(|s| s.to_string())
                .collect();

            // Write back to file
            ensure_shell_profile_path_allowed(&file_path)?;
            fs::write(&file_path, new_content.join("\n"))
                .map_err(|e| format!("写入文件失败 {}: {e}", file_path.display()))?;

            Ok(())
        }
        "system" => {
            // On Unix, we can't directly delete process environment variables
            Ok(())
        }
        _ => Err(format!("未知的环境变量来源类型: {}", conflict.source_type)),
    }
}

/// Restore environment variables from backup
pub fn restore_from_backup(backup_path: String) -> Result<(), String> {
    let backup_path = PathBuf::from(backup_path);
    let backup_root = get_backup_dir()?;
    if !crate::app_store::path_is_same_or_nested(&backup_path, &backup_root) {
        return Err("只能从魔改版自己的环境变量备份目录恢复".to_string());
    }
    crate::app_store::ensure_private_app_data_path_isolated(&backup_path)
        .map_err(|error| error.to_string())?;
    // Read backup file
    let content = fs::read_to_string(&backup_path).map_err(|e| format!("读取备份文件失败: {e}"))?;

    let backup_info: BackupInfo =
        serde_json::from_str(&content).map_err(|e| format!("解析备份文件失败: {e}"))?;
    ensure_conflicts_do_not_target_official_claude(&backup_info.conflicts)?;
    ensure_conflict_sources_are_safe(&backup_info.conflicts)?;

    // Restore each variable
    for conflict in &backup_info.conflicts {
        restore_single_env(conflict)?;
    }

    Ok(())
}

/// Restore a single environment variable
#[cfg(target_os = "windows")]
fn restore_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "system" => {
            if conflict.source_path.contains("HKEY_CURRENT_USER") {
                let (hkcu, _) = RegKey::predef(HKEY_CURRENT_USER)
                    .create_subkey("Environment")
                    .map_err(|e| format!("打开注册表失败: {}", e))?;

                hkcu.set_value(&conflict.var_name, &conflict.var_value)
                    .map_err(|e| format!("恢复注册表项失败: {}", e))?;
            } else if conflict.source_path.contains("HKEY_LOCAL_MACHINE") {
                let (hklm, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
                    .create_subkey(
                        "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                    )
                    .map_err(|e| format!("打开系统注册表失败 (需要管理员权限): {}", e))?;

                hklm.set_value(&conflict.var_name, &conflict.var_value)
                    .map_err(|e| format!("恢复系统注册表项失败: {}", e))?;
            }
            Ok(())
        }
        _ => Err(format!(
            "无法恢复类型为 {} 的环境变量",
            conflict.source_type
        )),
    }
}

#[cfg(not(target_os = "windows"))]
fn restore_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "file" => {
            // Parse file path from source_path
            let file_path = source_file_path(&conflict.source_path)?;
            ensure_shell_profile_path_allowed(&file_path)?;

            // Read file content
            let mut content = fs::read_to_string(&file_path)
                .map_err(|e| format!("读取文件失败 {}: {e}", file_path.display()))?;

            // Append the environment variable line
            let export_line = format!("\nexport {}={}", conflict.var_name, conflict.var_value);
            content.push_str(&export_line);

            // Write back to file
            ensure_shell_profile_path_allowed(&file_path)?;
            fs::write(&file_path, content)
                .map_err(|e| format!("写入文件失败 {}: {e}", file_path.display()))?;

            Ok(())
        }
        _ => Err(format!(
            "无法恢复类型为 {} 的环境变量",
            conflict.source_type
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict(var_name: &str) -> EnvConflict {
        EnvConflict {
            var_name: var_name.to_string(),
            var_value: "redacted".to_string(),
            source_type: "system".to_string(),
            source_path: "test".to_string(),
        }
    }

    #[test]
    fn private_fork_protects_official_claude_environment_variables() {
        assert!(
            ensure_conflicts_do_not_target_official_claude(&[conflict("ANTHROPIC_BASE_URL")])
                .is_err()
        );
        assert!(
            ensure_conflicts_do_not_target_official_claude(&[conflict("CLAUDE_CONFIG_DIR")])
                .is_err()
        );
        assert!(ensure_conflicts_do_not_target_official_claude(&[conflict(
            "CLAUDE_CODE_USE_BEDROCK"
        )])
        .is_err());
        assert!(
            ensure_conflicts_do_not_target_official_claude(&[conflict("DISABLE_AUTOUPDATER")])
                .is_err()
        );
        assert!(
            ensure_conflicts_do_not_target_official_claude(&[conflict("OPENAI_API_KEY")]).is_ok()
        );
    }

    #[test]
    fn test_backup_dir_creation() {
        let backup_dir = get_backup_dir();
        assert!(backup_dir.is_ok());
    }
}
