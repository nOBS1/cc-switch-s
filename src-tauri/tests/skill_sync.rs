use std::fs;

use cc_switch_lib::{
    migrate_skills_to_ssot, AppType, ImportSkillSelection, InstalledSkill, SkillApps, SkillService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(dir: &std::path::Path, name: &str) {
    fs::create_dir_all(dir).expect("create skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill\n---\n"),
    )
    .expect("write SKILL.md");
}

#[test]
fn private_fork_skill_sync_preserves_official_and_manages_cometix() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill = home
        .join(".cc-switch-cometix")
        .join("skills")
        .join("fork-skill");
    write_skill(&ssot_skill, "Fork Skill");
    fs::write(ssot_skill.join("prompt.md"), "managed-by-fork").expect("write SSOT prompt");

    let official_live = home.join(".claude").join("skills").join("fork-skill");
    fs::create_dir_all(official_live.parent().expect("official Skills parent"))
        .expect("create official Skills parent");
    let official_is_symlink = symlink_dir(&ssot_skill, &official_live);
    if !official_is_symlink {
        write_skill(&official_live, "Official Reference Fallback");
        fs::write(official_live.join("prompt.md"), "managed-by-fork")
            .expect("write official fallback prompt");
    }

    let cometix_live = home.join(".hlclaude").join("skills").join("fork-skill");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:fork-skill".to_string(),
            name: "Fork Skill".to_string(),
            description: None,
            directory: "fork-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                claude_cometix: true,
                ..Default::default()
            },
            installed_at: 0,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save fork skill");

    SkillService::sync_to_app(&state.db, &AppType::Claude)
        .expect("official Claude sync should be an inert compatibility call");
    SkillService::sync_to_app(&state.db, &AppType::ClaudeCometix)
        .expect("Cometix skill sync should remain enabled");

    assert_eq!(
        fs::read_to_string(official_live.join("prompt.md")).expect("read official live prompt"),
        "managed-by-fork",
        "the private fork must not replace official Claude Skills"
    );
    assert_eq!(
        fs::read_to_string(cometix_live.join("prompt.md")).expect("read Cometix live prompt"),
        "managed-by-fork",
        "the same unified Skill operation must still project to Cometix"
    );

    SkillService::uninstall(&state.db, "local:fork-skill").expect("uninstall managed targets");
    assert!(
        official_live.join("prompt.md").exists(),
        "global uninstall must not break the official Claude Skill reference"
    );
    assert!(
        ssot_skill.join("prompt.md").exists(),
        "the legacy shared SSOT must remain for official Claude"
    );
    if official_is_symlink {
        assert!(
            fs::symlink_metadata(&official_live)
                .expect("inspect official live Skill")
                .file_type()
                .is_symlink(),
            "the existing official Claude symlink must remain intact"
        );
    }
    assert!(
        !cometix_live.exists(),
        "global uninstall must still remove the Cometix Skill"
    );
    let preserved = state
        .db
        .get_installed_skill("local:fork-skill")
        .expect("query preserved official row")
        .expect("official-only legacy row must remain");
    assert!(preserved.apps.claude);
    assert!(!preserved.apps.claude_cometix);
}

#[test]
fn private_fork_rejects_uninstall_of_official_only_legacy_skill() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill = home
        .join(".cc-switch-cometix")
        .join("skills")
        .join("official-only");
    write_skill(&ssot_skill, "Official Only");
    fs::write(ssot_skill.join("prompt.md"), "official-owned").expect("write official prompt");

    let official_live = home.join(".claude").join("skills").join("official-only");
    fs::create_dir_all(official_live.parent().expect("official Skills parent"))
        .expect("create official Skills parent");
    let official_is_symlink = symlink_dir(&ssot_skill, &official_live);

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:official-only".to_string(),
            name: "Official Only".to_string(),
            description: None,
            directory: "official-only".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                ..Default::default()
            },
            installed_at: 0,
            content_hash: None,
            updated_at: 0,
        })
        .expect("seed official-only row");

    let error = SkillService::uninstall(&state.db, "local:official-only")
        .expect_err("official-only uninstall must be rejected");
    assert!(
        error.to_string().contains("Official Claude Code"),
        "the rejection should explain the private-fork ownership boundary: {error:#}"
    );
    assert!(ssot_skill.join("prompt.md").exists());
    assert!(
        state
            .db
            .get_installed_skill("local:official-only")
            .expect("query official row")
            .is_some(),
        "a rejected uninstall must keep the database row"
    );
    if official_is_symlink {
        assert_eq!(
            fs::read_to_string(official_live.join("prompt.md")).expect("read official symlink"),
            "official-owned"
        );
    }
}

#[test]
fn private_fork_rejects_update_of_official_only_legacy_skill_before_download() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill = home
        .join(".cc-switch-cometix")
        .join("skills")
        .join("official-update");
    write_skill(&ssot_skill, "Official Update");
    fs::write(ssot_skill.join("prompt.md"), "official-v1").expect("write official prompt");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "owner/repo:official-update".to_string(),
            name: "Official Update".to_string(),
            description: None,
            directory: "official-update".to_string(),
            repo_owner: Some("invalid owner".to_string()),
            repo_name: Some("repo".to_string()),
            repo_branch: Some("main".to_string()),
            readme_url: None,
            apps: SkillApps {
                claude: true,
                ..Default::default()
            },
            installed_at: 0,
            content_hash: None,
            updated_at: 0,
        })
        .expect("seed official-only update row");

    let error = futures::executor::block_on(
        SkillService::new().update_skill(&state.db, "owner/repo:official-update"),
    )
    .expect_err("official-only update must be rejected");
    assert!(
        error.to_string().contains("Official Claude Code"),
        "the ownership guard must run before repository validation or download: {error:#}"
    );
    assert_eq!(
        fs::read_to_string(ssot_skill.join("prompt.md")).expect("read preserved official source"),
        "official-v1"
    );
}

#[test]
fn private_fork_global_skill_scan_skips_official_and_keeps_cometix() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("official-only"),
        "Official Only",
    );
    write_skill(
        &home.join(".hlclaude").join("skills").join("cometix-only"),
        "Cometix Only",
    );

    let state = create_test_state().expect("create test state");
    let unmanaged = SkillService::scan_unmanaged(&state.db).expect("scan unmanaged Skills");

    assert!(
        unmanaged.iter().all(|skill| {
            skill.directory != "official-only"
                && skill.found_in.iter().all(|source| source != "claude")
        }),
        "the private fork must not discover official Claude Skills"
    );
    assert!(
        unmanaged.iter().any(|skill| {
            skill.directory == "cometix-only"
                && skill
                    .found_in
                    .iter()
                    .any(|source| source == "claude-cometix")
        }),
        "the same global scan must still discover Cometix Skills"
    );
}

#[test]
fn private_fork_unified_skill_import_filters_official_and_keeps_cometix() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("same-skill"),
        "Official Same Skill",
    );
    write_skill(
        &home.join(".hlclaude").join("skills").join("same-skill"),
        "Cometix Same Skill",
    );

    let state = create_test_state().expect("create test state");
    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "same-skill".to_string(),
            apps: SkillApps {
                claude: true,
                claude_cometix: true,
                ..Default::default()
            },
        }],
    )
    .expect("import selected Skills");

    assert_eq!(
        imported.len(),
        1,
        "only the allowed Cometix scope is imported"
    );
    assert!(imported[0].apps.claude_cometix);
    assert!(!imported[0].apps.claude);
    assert_eq!(imported[0].name, "Cometix Same Skill");
}

#[cfg(unix)]
fn symlink_dir(src: &std::path::Path, dest: &std::path::Path) -> bool {
    std::os::unix::fs::symlink(src, dest).expect("create symlink");
    true
}

#[cfg(windows)]
fn symlink_dir(src: &std::path::Path, dest: &std::path::Path) -> bool {
    std::os::windows::fs::symlink_dir(src, dest).is_ok()
}

#[test]
fn import_from_apps_respects_explicit_app_selection() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("shared-skill"),
        "Shared",
    );
    write_skill(
        &home
            .join(".config")
            .join("opencode")
            .join("skills")
            .join("shared-skill"),
        "Shared",
    );

    let state = create_test_state().expect("create test state");

    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "shared-skill".to_string(),
            apps: SkillApps {
                opencode: true,
                ..Default::default()
            },
        }],
    )
    .expect("import skills");

    assert_eq!(imported.len(), 1, "expected exactly one imported skill");
    let skill = imported.first().expect("imported skill");
    assert!(
        skill.apps.opencode,
        "explicitly selected OpenCode app should remain enabled"
    );
    assert!(
        !skill.apps.claude && !skill.apps.codex && !skill.apps.gemini,
        "import should no longer infer apps from every matching source path"
    );
}

#[test]
fn import_from_apps_does_not_rewrite_selected_app_directory() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home
        .join(".cc-switch-cometix")
        .join("skills")
        .join("codex-skill");
    write_skill(&ssot_skill_dir, "Stale SSOT Skill");
    fs::write(ssot_skill_dir.join("prompt.md"), "stale ssot").expect("write stale ssot prompt");

    let codex_skill_dir = home.join(".codex").join("skills").join("codex-skill");
    write_skill(&codex_skill_dir, "Live Codex Skill");
    fs::write(codex_skill_dir.join("prompt.md"), "live codex").expect("write live codex prompt");

    let state = create_test_state().expect("create test state");

    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "codex-skill".to_string(),
            apps: SkillApps {
                codex: true,
                ..Default::default()
            },
        }],
    )
    .expect("import skills");

    assert_eq!(imported.len(), 1, "expected exactly one imported skill");
    assert!(
        imported[0].apps.codex,
        "import should preserve the selected Codex app state"
    );
    assert_eq!(
        fs::read_to_string(codex_skill_dir.join("prompt.md")).expect("read live codex prompt"),
        "live codex",
        "import should not replace the app skill directory with SSOT contents"
    );
    assert!(
        !fs::symlink_metadata(&codex_skill_dir)
            .expect("read codex skill metadata")
            .file_type()
            .is_symlink(),
        "import should not replace the app skill directory with a managed symlink"
    );
}

#[test]
fn claude_and_cometix_same_named_skills_are_independent() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let claude_live = home.join(".claude").join("skills").join("same-skill");
    write_skill(&claude_live, "Official Same Skill");
    fs::write(claude_live.join("prompt.md"), "official-v1").expect("write official prompt");

    let cometix_live = home.join(".hlclaude").join("skills").join("same-skill");
    write_skill(&cometix_live, "Cometix Same Skill");
    fs::write(cometix_live.join("prompt.md"), "cometix-v1").expect("write cometix prompt");

    let state = create_test_state().expect("create test state");
    let official_import = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "same-skill".to_string(),
            apps: SkillApps {
                claude: true,
                ..Default::default()
            },
        }],
    )
    .expect("import official Claude domain");
    assert!(
        official_import.is_empty(),
        "the private fork must not import official Claude Skills"
    );

    let unmanaged_after_official = SkillService::scan_unmanaged(&state.db)
        .expect("scan Cometix after importing official Claude");
    assert!(
        unmanaged_after_official.iter().any(|skill| {
            skill.directory == "same-skill"
                && skill.found_in.iter().any(|app| app == "claude-cometix")
        }),
        "the same-named Cometix live skill must remain independently importable"
    );

    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "same-skill".to_string(),
            apps: SkillApps {
                claude_cometix: true,
                ..Default::default()
            },
        }],
    )
    .expect("import Cometix Claude domain");

    assert_eq!(
        imported.len(),
        1,
        "only the independently managed Cometix domain is imported"
    );
    let cometix = imported
        .iter()
        .find(|skill| skill.apps.claude_cometix)
        .expect("Cometix record");

    let ssot = SkillService::get_ssot_dir().expect("resolve SSOT");
    let cometix_ssot = ssot
        .join(".scopes")
        .join("claude-cometix")
        .join("same-skill");
    assert_eq!(
        fs::read_to_string(cometix_ssot.join("prompt.md")).expect("read Cometix SSOT"),
        "cometix-v1"
    );

    SkillService::sync_to_app(&state.db, &AppType::Claude).expect("sync official Claude");
    assert_eq!(
        fs::read_to_string(claude_live.join("prompt.md")).expect("read official live"),
        "official-v1",
        "official Claude content remains owned by the upstream app"
    );
    assert_eq!(
        fs::read_to_string(cometix_live.join("prompt.md")).expect("read Cometix live"),
        "cometix-v1",
        "syncing official Claude must not rewrite Cometix"
    );

    SkillService::uninstall(&state.db, &cometix.id).expect("uninstall Cometix skill");
    assert!(
        state
            .db
            .get_installed_skill(&cometix.id)
            .expect("query Cometix row")
            .is_none(),
        "the managed Cometix DB row should be removed"
    );
    assert_eq!(
        fs::read_to_string(claude_live.join("prompt.md"))
            .expect("read official live after Cometix uninstall"),
        "official-v1",
        "uninstalling Cometix must preserve official Claude content"
    );
}

#[test]
fn legacy_shared_claude_skill_is_split_without_losing_diverged_live_contents() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let ssot = SkillService::get_ssot_dir().expect("resolve SSOT");

    let shared = ssot.join("legacy-skill");
    write_skill(&shared, "Legacy Shared");
    fs::write(shared.join("prompt.md"), "shared").expect("write shared prompt");

    let official_live = home.join(".claude").join("skills").join("legacy-skill");
    write_skill(&official_live, "Legacy Official");
    fs::write(official_live.join("prompt.md"), "official-live")
        .expect("write official live prompt");

    let cometix_live = home.join(".hlclaude").join("skills").join("legacy-skill");
    write_skill(&cometix_live, "Legacy Cometix");
    fs::write(cometix_live.join("prompt.md"), "cometix-live").expect("write Cometix live prompt");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:legacy-skill".to_string(),
            name: "Legacy Shared".to_string(),
            description: None,
            directory: "legacy-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                claude_cometix: true,
                ..Default::default()
            },
            installed_at: 42,
            content_hash: None,
            updated_at: 0,
        })
        .expect("seed legacy shared row");

    assert_eq!(
        SkillService::migrate_claude_scopes(&state.db).expect("migrate Claude scopes"),
        1
    );
    assert!(
        state
            .db
            .get_installed_skill("local:legacy-skill")
            .expect("query legacy row")
            .is_some(),
        "the official-only legacy row remains owned by the upstream app"
    );
    let rows = state
        .db
        .get_all_installed_skills()
        .expect("query scoped rows");
    assert_eq!(rows.len(), 2);
    assert!(rows.values().any(|skill| skill.apps.claude));
    assert!(rows.values().any(|skill| skill.apps.claude_cometix));
    assert_eq!(
        fs::read_to_string(shared.join("prompt.md")).expect("read preserved shared prompt"),
        "shared",
        "the official legacy source must not be rewritten or moved"
    );
    assert_eq!(
        fs::read_to_string(official_live.join("prompt.md")).expect("read official live prompt"),
        "official-live"
    );
    assert_eq!(
        fs::read_to_string(
            ssot.join(".scopes")
                .join("claude-cometix")
                .join("legacy-skill")
                .join("prompt.md"),
        )
        .expect("read migrated Cometix prompt"),
        "cometix-live"
    );
}

#[test]
fn sync_to_app_removes_disabled_and_orphaned_ssot_symlinks() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_dir = home.join(".cc-switch-cometix").join("skills");
    let disabled_skill = ssot_dir.join("disabled-skill");
    let orphan_skill = ssot_dir.join("orphan-skill");
    write_skill(&disabled_skill, "Disabled");
    write_skill(&orphan_skill, "Orphan");

    let opencode_skills_dir = home.join(".config").join("opencode").join("skills");
    fs::create_dir_all(&opencode_skills_dir).expect("create opencode skills dir");
    if !symlink_dir(&disabled_skill, &opencode_skills_dir.join("disabled-skill"))
        || !symlink_dir(&orphan_skill, &opencode_skills_dir.join("orphan-skill"))
    {
        // Windows may require Developer Mode or SeCreateSymbolicLinkPrivilege.
        return;
    }

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:disabled-skill".to_string(),
            name: "Disabled".to_string(),
            description: None,
            directory: "disabled-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps::default(),
            installed_at: 0,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save disabled skill");

    SkillService::sync_to_app(&state.db, &AppType::OpenCode).expect("reconcile skills");

    assert!(
        !opencode_skills_dir.join("disabled-skill").exists(),
        "DB-known disabled skill should be removed from OpenCode live dir"
    );
    assert!(
        !opencode_skills_dir.join("orphan-skill").exists(),
        "orphaned symlink into SSOT should be cleaned up"
    );
}

#[test]
fn uninstall_skill_creates_backup_before_removing_ssot() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home
        .join(".cc-switch-cometix")
        .join("skills")
        .join("backup-skill");
    write_skill(&ssot_skill_dir, "Backup Skill");
    fs::write(ssot_skill_dir.join("prompt.md"), "backup me").expect("write prompt.md");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:backup-skill".to_string(),
            name: "Backup Skill".to_string(),
            description: Some("Back me up before uninstall".to_string()),
            directory: "backup-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                codex: true,
                ..Default::default()
            },
            installed_at: 123,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let result = SkillService::uninstall(&state.db, "local:backup-skill").expect("uninstall skill");
    let backup_path = result.backup_path.expect("backup path should be returned");
    let backup_dir = std::path::PathBuf::from(&backup_path);

    assert!(backup_dir.exists(), "backup directory should exist");
    assert!(
        backup_dir.join("skill").join("SKILL.md").exists(),
        "backup should include SKILL.md"
    );
    assert_eq!(
        fs::read_to_string(backup_dir.join("skill").join("prompt.md"))
            .expect("read backed up prompt"),
        "backup me"
    );

    let metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(backup_dir.join("meta.json")).expect("read backup metadata"),
    )
    .expect("parse backup metadata");
    assert_eq!(metadata["skill"]["directory"], "backup-skill");
    assert_eq!(metadata["skill"]["name"], "Backup Skill");

    assert!(
        !ssot_skill_dir.exists(),
        "SSOT skill directory should be removed after uninstall"
    );
    assert!(
        state
            .db
            .get_installed_skill("local:backup-skill")
            .expect("query skill")
            .is_none(),
        "database row should be deleted after uninstall"
    );
}

#[test]
fn restore_skill_backup_restores_files_to_ssot_and_current_app() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home
        .join(".cc-switch-cometix")
        .join("skills")
        .join("restore-skill");
    write_skill(&ssot_skill_dir, "Restore Skill");
    fs::write(ssot_skill_dir.join("prompt.md"), "restore me").expect("write prompt.md");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:restore-skill".to_string(),
            name: "Restore Skill".to_string(),
            description: Some("Bring the files back".to_string()),
            directory: "restore-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                codex: true,
                ..Default::default()
            },
            installed_at: 456,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let uninstall =
        SkillService::uninstall(&state.db, "local:restore-skill").expect("uninstall skill");
    let backup_id = std::path::Path::new(
        &uninstall
            .backup_path
            .expect("backup path should be returned on uninstall"),
    )
    .file_name()
    .expect("backup dir name")
    .to_string_lossy()
    .to_string();

    let restored = SkillService::restore_from_backup(&state.db, &backup_id, &AppType::Codex)
        .expect("restore from backup");

    assert_eq!(restored.directory, "restore-skill");
    assert!(restored.apps.codex, "restored skill should enable Codex");
    assert!(
        !restored.apps.claude && !restored.apps.gemini && !restored.apps.opencode,
        "restore should only enable the selected app"
    );
    assert!(
        SkillService::get_ssot_dir()
            .expect("resolve SSOT")
            .join("restore-skill")
            .join("prompt.md")
            .exists(),
        "restored Codex skill should exist in the managed SSOT"
    );
    assert!(
        home.join(".codex")
            .join("skills")
            .join("restore-skill")
            .join("prompt.md")
            .exists(),
        "restored skill should sync to the selected app"
    );
    assert!(
        state
            .db
            .get_installed_skill(&restored.id)
            .expect("query restored skill")
            .is_some(),
        "restored skill should be written back to the database"
    );
}

#[test]
fn delete_skill_backup_removes_backup_directory() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home
        .join(".cc-switch-cometix")
        .join("skills")
        .join("delete-backup-skill");
    write_skill(&ssot_skill_dir, "Delete Backup Skill");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:delete-backup-skill".to_string(),
            name: "Delete Backup Skill".to_string(),
            description: Some("Remove my backup".to_string()),
            directory: "delete-backup-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                codex: true,
                ..Default::default()
            },
            installed_at: 789,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let uninstall =
        SkillService::uninstall(&state.db, "local:delete-backup-skill").expect("uninstall skill");
    let backup_path = uninstall
        .backup_path
        .expect("backup path should be returned on uninstall");
    let backup_id = std::path::Path::new(&backup_path)
        .file_name()
        .expect("backup dir name")
        .to_string_lossy()
        .to_string();

    assert!(
        std::path::Path::new(&backup_path).exists(),
        "backup directory should exist before deletion"
    );

    SkillService::delete_backup(&backup_id).expect("delete backup");

    assert!(
        !std::path::Path::new(&backup_path).exists(),
        "backup directory should be removed"
    );
    assert!(
        SkillService::list_backups()
            .expect("list backups")
            .into_iter()
            .all(|entry| entry.backup_id != backup_id),
        "deleted backup should no longer appear in backup list"
    );
}

#[test]
fn migration_snapshot_overrides_multi_source_directory_inference() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".codex").join("skills").join("demo-skill"),
        "Demo",
    );
    write_skill(
        &home
            .join(".config")
            .join("opencode")
            .join("skills")
            .join("demo-skill"),
        "Demo",
    );

    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"demo-skill","app_type":"codex"}]"#,
        )
        .expect("seed migration snapshot");

    let count = migrate_skills_to_ssot(&state.db).expect("migrate skills to ssot");
    assert_eq!(count, 1, "expected one migrated skill");

    let skills = state.db.get_all_installed_skills().expect("get skills");
    let migrated = skills
        .values()
        .find(|skill| skill.directory == "demo-skill")
        .expect("migrated demo-skill");

    assert!(
        migrated.apps.codex,
        "legacy snapshot should preserve Codex enablement"
    );
    assert!(
        !migrated.apps.opencode,
        "migration should no longer infer OpenCode enablement from a duplicate directory alone"
    );
}
