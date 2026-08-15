//! Migration from the early `.claude-cometix` development directory to the
//! configuration domain actually consumed by `hlclaude` (`.hlclaude`).
//!
//! The source is deliberately retained. Existing target data always wins so a
//! migration can be retried safely without combining two different versions of
//! the same MCP server, prompt, or skill.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::{atomic_write, read_json_file, write_json_file};
use crate::error::AppError;

const SKILL_STAGING_PREFIX: &str = ".cc-switch-cometix-skill-migration";
static SKILL_STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CometixAuxiliaryMigrationOutcome {
    pub mcp_servers_added: usize,
    pub prompt_copied: bool,
    pub skills_copied: usize,
}

pub fn migrate_legacy_cometix_auxiliary_assets(
) -> Result<CometixAuxiliaryMigrationOutcome, AppError> {
    let home = crate::config::get_home_dir();
    migrate_legacy_cometix_auxiliary_assets_once_at(
        &home.join(".claude-cometix"),
        &crate::config::get_claude_cometix_config_dir(),
        &crate::config::get_app_config_dir(),
    )
}

fn auxiliary_migration_marker_path(marker_root: &Path, target_dir: &Path) -> PathBuf {
    let target = target_dir.to_string_lossy();
    let target_hash = format!("{:x}", Sha256::digest(target.as_bytes()));
    marker_root
        .join("local-migrations")
        .join("cometix-hlclaude-auxiliary-v1")
        .join(format!("{target_hash}.complete"))
}

fn auxiliary_migration_is_complete(marker_root: &Path, target_dir: &Path) -> bool {
    let marker = auxiliary_migration_marker_path(marker_root, target_dir);
    fs::read_to_string(marker)
        .map(|recorded_target| recorded_target == target_dir.to_string_lossy())
        .unwrap_or(false)
}

fn migrate_legacy_cometix_auxiliary_assets_once_at(
    legacy_dir: &Path,
    target_dir: &Path,
    marker_root: &Path,
) -> Result<CometixAuxiliaryMigrationOutcome, AppError> {
    if auxiliary_migration_is_complete(marker_root, target_dir) {
        return Ok(Default::default());
    }

    let outcome = migrate_legacy_cometix_auxiliary_assets_at(legacy_dir, target_dir)?;
    let marker = auxiliary_migration_marker_path(marker_root, target_dir);
    atomic_write(&marker, target_dir.to_string_lossy().as_bytes())?;
    Ok(outcome)
}

fn merge_missing(target: &mut Value, source: &Value) -> usize {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return 0;
    };

    let mut added = 0;
    for (key, source_value) in source {
        if let Some(target_value) = target.get_mut(key) {
            added += merge_missing(target_value, source_value);
        } else {
            target.insert(key.clone(), source_value.clone());
            added += 1;
        }
    }
    added
}

fn merge_legacy_mcp(source: &Path, target: &Path) -> Result<usize, AppError> {
    if !source.exists() {
        return Ok(0);
    }

    let source_json = read_json_file(source)?;
    let mut target_json = if target.exists() {
        read_json_file(target)?
    } else {
        serde_json::json!({})
    };

    let before = target_json
        .get("mcpServers")
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len);
    merge_missing(&mut target_json, &source_json);
    let after = target_json
        .get("mcpServers")
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len);

    if after != before || !target.exists() {
        write_json_file(target, &target_json)?;
    }
    Ok(after.saturating_sub(before))
}

fn copy_dir_without_links(source: &Path, target: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|error| AppError::io(target, error))?;
    let entries = fs::read_dir(source).map_err(|error| AppError::io(source, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| AppError::io(source, error))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| AppError::io(&source_path, error))?;

        if metadata.file_type().is_symlink() {
            log::warn!(
                "Skipping symlink while migrating legacy Cometix skill: {}",
                source_path.display()
            );
        } else if metadata.is_dir() {
            copy_dir_without_links(&source_path, &target_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &target_path)
                .map_err(|error| AppError::io(&target_path, error))?;
        }
    }
    Ok(())
}

fn create_unique_skill_staging_dir(parent: &Path) -> Result<PathBuf, AppError> {
    fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;

    for _ in 0..32 {
        let sequence = SKILL_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let staging = parent.join(format!(
            "{SKILL_STAGING_PREFIX}-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(AppError::io(&staging, error)),
        }
    }

    Err(AppError::Message(format!(
        "Unable to allocate a unique Cometix skill migration directory under {}",
        parent.display()
    )))
}

fn cleanup_skill_staging_dir(staging: &Path) -> Result<(), AppError> {
    if staging.exists() {
        fs::remove_dir_all(staging).map_err(|error| AppError::io(staging, error))?;
    }
    Ok(())
}

fn publish_skill_atomically_with<F>(target_skill: &Path, copy_into: F) -> Result<bool, AppError>
where
    F: FnOnce(&Path) -> Result<(), AppError>,
{
    // The independently managed target always wins, including a target created
    // by another process between discovery and migration.
    if target_skill.exists() {
        return Ok(false);
    }

    let parent = target_skill.parent().ok_or_else(|| {
        AppError::Message(format!(
            "Cometix skill target has no parent: {}",
            target_skill.display()
        ))
    })?;
    let staging = create_unique_skill_staging_dir(parent)?;

    if let Err(copy_error) = copy_into(&staging) {
        if let Err(cleanup_error) = cleanup_skill_staging_dir(&staging) {
            return Err(AppError::Message(format!(
                "Cometix skill migration failed ({copy_error}); temporary directory cleanup also failed ({cleanup_error})"
            )));
        }
        return Err(copy_error);
    }

    match fs::rename(&staging, target_skill) {
        Ok(()) => Ok(true),
        Err(_) if target_skill.exists() => {
            cleanup_skill_staging_dir(&staging)?;
            Ok(false)
        }
        Err(rename_error) => {
            if let Err(cleanup_error) = cleanup_skill_staging_dir(&staging) {
                return Err(AppError::Message(format!(
                    "Publishing migrated Cometix skill failed ({rename_error}); temporary directory cleanup also failed ({cleanup_error})"
                )));
            }
            Err(AppError::io(target_skill, rename_error))
        }
    }
}

fn publish_skill_atomically(source_skill: &Path, target_skill: &Path) -> Result<bool, AppError> {
    publish_skill_atomically_with(target_skill, |staging| {
        copy_dir_without_links(source_skill, staging)
    })
}

fn copy_legacy_skills(source: &Path, target: &Path) -> Result<usize, AppError> {
    if !source.is_dir() {
        return Ok(0);
    }

    let mut copied = 0;
    for entry in fs::read_dir(source).map_err(|error| AppError::io(source, error))? {
        let entry = entry.map_err(|error| AppError::io(source, error))?;
        let source_skill = entry.path();
        let metadata = fs::symlink_metadata(&source_skill)
            .map_err(|error| AppError::io(&source_skill, error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }

        let manifest = source_skill.join("SKILL.md");
        let manifest_is_regular = fs::symlink_metadata(&manifest)
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false);
        if !manifest_is_regular {
            continue;
        }

        let target_skill = target.join(entry.file_name());
        if target_skill.exists() {
            continue;
        }
        if publish_skill_atomically(&source_skill, &target_skill)? {
            copied += 1;
        }
    }
    Ok(copied)
}

pub(crate) fn migrate_legacy_cometix_auxiliary_assets_at(
    legacy_dir: &Path,
    target_dir: &Path,
) -> Result<CometixAuxiliaryMigrationOutcome, AppError> {
    if !legacy_dir.exists() {
        return Ok(Default::default());
    }

    let mcp_servers_added = merge_legacy_mcp(
        &legacy_dir.join(".claude.json"),
        &target_dir.join(".claude.json"),
    )?;

    let legacy_prompt = legacy_dir.join("CLAUDE.md");
    let target_prompt = target_dir.join("CLAUDE.md");
    let prompt_copied = if legacy_prompt.is_file() && !target_prompt.exists() {
        let bytes =
            fs::read(&legacy_prompt).map_err(|error| AppError::io(&legacy_prompt, error))?;
        atomic_write(&target_prompt, &bytes)?;
        true
    } else {
        false
    };

    let skills_copied = copy_legacy_skills(&legacy_dir.join("skills"), &target_dir.join("skills"))?;

    Ok(CometixAuxiliaryMigrationOutcome {
        mcp_servers_added,
        prompt_copied,
        skills_copied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_auxiliary_migration_does_not_write_completion_marker() {
        let temp = tempfile::tempdir().expect("temp dir");
        let legacy = temp.path().join(".claude-cometix");
        let target = temp.path().join(".hlclaude");
        let marker_root = temp.path().join("app-data");

        fs::create_dir_all(legacy.join("skills/legacy-skill")).expect("legacy skill dir");
        fs::write(legacy.join("skills/legacy-skill/SKILL.md"), "legacy skill")
            .expect("legacy skill");
        fs::create_dir_all(&target).expect("target dir");
        fs::write(target.join("skills"), "blocks the skills directory")
            .expect("blocking target file");

        assert!(
            migrate_legacy_cometix_auxiliary_assets_once_at(&legacy, &target, &marker_root)
                .is_err()
        );
        assert!(!auxiliary_migration_marker_path(&marker_root, &target).exists());

        fs::remove_file(target.join("skills")).expect("remove blocking target file");
        let retry = migrate_legacy_cometix_auxiliary_assets_once_at(&legacy, &target, &marker_root)
            .expect("retry migration");
        assert_eq!(retry.skills_copied, 1);
        assert!(auxiliary_migration_marker_path(&marker_root, &target).is_file());
    }

    #[test]
    fn failed_atomic_skill_publish_cleans_partial_stage_and_retry_succeeds() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source = temp.path().join("legacy-skill");
        let target_parent = temp.path().join("target-skills");
        let target = target_parent.join("legacy-skill");

        fs::create_dir_all(source.join("nested")).expect("source directories");
        fs::write(source.join("SKILL.md"), "legacy skill").expect("source manifest");
        fs::write(source.join("nested/data.txt"), "complete payload").expect("source payload");

        let failed = publish_skill_atomically_with(&target, |staging| {
            fs::write(staging.join("SKILL.md"), "partial payload")
                .map_err(|error| AppError::io(staging, error))?;
            Err(AppError::Message("injected copy failure".to_string()))
        });
        assert!(failed.is_err());
        assert!(!target.exists(), "a partial target must never be published");
        assert!(
            fs::read_dir(&target_parent)
                .expect("read target parent")
                .next()
                .is_none(),
            "the failed staging directory must be removed"
        );

        assert!(
            publish_skill_atomically(&source, &target).expect("retry atomic publish"),
            "retry should publish the complete skill"
        );
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).expect("published manifest"),
            "legacy skill"
        );
        assert_eq!(
            fs::read_to_string(target.join("nested/data.txt")).expect("published payload"),
            "complete payload"
        );
    }

    #[test]
    fn completed_auxiliary_migration_does_not_restore_deleted_target_assets() {
        let temp = tempfile::tempdir().expect("temp dir");
        let legacy = temp.path().join(".claude-cometix");
        let target = temp.path().join(".hlclaude");
        let marker_root = temp.path().join("app-data");

        fs::create_dir_all(legacy.join("skills/legacy-skill")).expect("legacy skill dir");
        write_json_file(
            &legacy.join(".claude.json"),
            &serde_json::json!({
                "mcpServers": {"legacy-server": {"command": "legacy"}}
            }),
        )
        .expect("legacy MCP");
        fs::write(legacy.join("CLAUDE.md"), "legacy prompt").expect("legacy prompt");
        fs::write(legacy.join("skills/legacy-skill/SKILL.md"), "legacy skill")
            .expect("legacy skill");

        let first = migrate_legacy_cometix_auxiliary_assets_once_at(&legacy, &target, &marker_root)
            .expect("first migration");
        assert_eq!(first.mcp_servers_added, 1);
        assert!(first.prompt_copied);
        assert_eq!(first.skills_copied, 1);

        fs::remove_file(target.join(".claude.json")).expect("remove migrated MCP");
        fs::remove_file(target.join("CLAUDE.md")).expect("remove migrated prompt");
        fs::remove_dir_all(target.join("skills/legacy-skill")).expect("remove migrated skill");

        let second =
            migrate_legacy_cometix_auxiliary_assets_once_at(&legacy, &target, &marker_root)
                .expect("second migration");
        assert_eq!(second, CometixAuxiliaryMigrationOutcome::default());
        assert!(!target.join(".claude.json").exists());
        assert!(!target.join("CLAUDE.md").exists());
        assert!(!target.join("skills/legacy-skill").exists());
        assert!(legacy.join(".claude.json").exists(), "source is retained");

        let other_target = temp.path().join("custom-cometix-profile");
        let other =
            migrate_legacy_cometix_auxiliary_assets_once_at(&legacy, &other_target, &marker_root)
                .expect("a different target has its own migration marker");
        assert_eq!(other.mcp_servers_added, 1);
        assert!(other.prompt_copied);
        assert_eq!(other.skills_copied, 1);
    }

    #[test]
    fn migration_preserves_target_and_only_adds_missing_auxiliary_assets() {
        let temp = tempfile::tempdir().expect("temp dir");
        let legacy = temp.path().join(".claude-cometix");
        let target = temp.path().join(".hlclaude");
        fs::create_dir_all(legacy.join("skills/same")).expect("legacy same skill");
        fs::create_dir_all(legacy.join("skills/legacy-only")).expect("legacy-only skill");
        fs::create_dir_all(target.join("skills/same")).expect("target same skill");
        fs::write(legacy.join("skills/same/SKILL.md"), "legacy same").expect("write legacy same");
        fs::write(legacy.join("skills/legacy-only/SKILL.md"), "legacy only")
            .expect("write legacy-only");
        fs::write(target.join("skills/same/SKILL.md"), "target same").expect("write target same");

        write_json_file(
            &legacy.join(".claude.json"),
            &serde_json::json!({
                "mcpServers": {
                    "same": {"command": "legacy"},
                    "legacy-only": {"command": "legacy-only"}
                },
                "legacyPreference": true
            }),
        )
        .expect("write legacy MCP");
        write_json_file(
            &target.join(".claude.json"),
            &serde_json::json!({
                "mcpServers": {
                    "same": {"command": "target"},
                    "target-only": {"command": "target-only"}
                },
                "targetPreference": true
            }),
        )
        .expect("write target MCP");
        fs::write(legacy.join("CLAUDE.md"), "legacy prompt").expect("legacy prompt");
        fs::write(target.join("CLAUDE.md"), "target prompt").expect("target prompt");

        let outcome = migrate_legacy_cometix_auxiliary_assets_at(&legacy, &target)
            .expect("migrate auxiliary assets");
        assert_eq!(
            outcome,
            CometixAuxiliaryMigrationOutcome {
                mcp_servers_added: 1,
                prompt_copied: false,
                skills_copied: 1,
            }
        );

        let mcp: Value = read_json_file(&target.join(".claude.json")).expect("read target MCP");
        assert_eq!(mcp["mcpServers"]["same"]["command"], "target");
        assert_eq!(mcp["mcpServers"]["legacy-only"]["command"], "legacy-only");
        assert_eq!(mcp["mcpServers"]["target-only"]["command"], "target-only");
        assert_eq!(mcp["legacyPreference"], true);
        assert_eq!(mcp["targetPreference"], true);
        assert_eq!(
            fs::read_to_string(target.join("CLAUDE.md")).expect("target prompt remains"),
            "target prompt"
        );
        assert_eq!(
            fs::read_to_string(target.join("skills/same/SKILL.md")).expect("target same remains"),
            "target same"
        );
        assert_eq!(
            fs::read_to_string(target.join("skills/legacy-only/SKILL.md"))
                .expect("legacy-only copied"),
            "legacy only"
        );

        let second = migrate_legacy_cometix_auxiliary_assets_at(&legacy, &target)
            .expect("migration is idempotent");
        assert_eq!(second, Default::default());
        assert!(legacy.exists(), "legacy source must never be removed");
    }

    #[test]
    fn migration_copies_prompt_when_target_is_missing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let legacy = temp.path().join("legacy");
        let target = temp.path().join("target");
        fs::create_dir_all(&legacy).expect("legacy dir");
        fs::write(legacy.join("CLAUDE.md"), b"legacy prompt\r\n").expect("write prompt");

        let outcome =
            migrate_legacy_cometix_auxiliary_assets_at(&legacy, &target).expect("migrate prompt");
        assert!(outcome.prompt_copied);
        assert_eq!(
            fs::read(target.join("CLAUDE.md")).expect("read copied prompt"),
            b"legacy prompt\r\n"
        );
    }
}
