use super::*;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

#[derive(Default)]
struct MockState {
    run: Option<String>,
    blocked: Option<bool>,
    reads: usize,
    writes: Vec<Option<String>>,
    registered: HashMap<i32, Hotkey>,
    attempts: Vec<(i32, String)>,
    unregistered: Vec<i32>,
    conflicts: HashSet<String>,
    fail_write: bool,
    fail_unregister: HashSet<i32>,
    external_after_write: Option<String>,
    external_on_read: Option<(usize, String)>,
}

#[derive(Clone, Default)]
struct MockPlatform(Arc<Mutex<MockState>>);

impl Platform for MockPlatform {
    fn read_run(&self) -> Result<Option<String>, String> {
        let mut state = self.0.lock().unwrap();
        state.reads += 1;
        if state
            .external_on_read
            .as_ref()
            .is_some_and(|(at, _)| *at == state.reads)
        {
            state.run = state.external_on_read.take().map(|(_, value)| value);
        }
        Ok(state.run.clone())
    }
    fn write_run(&self, value: Option<&str>) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        state.writes.push(value.map(str::to_string));
        if state.fail_write {
            return Err("registry fixture denied write".into());
        }
        state.run = value.map(str::to_string);
        if let Some(external) = state.external_after_write.take() {
            state.run = Some(external);
        }
        Ok(())
    }
    fn startup_blocked(&self) -> Result<Option<bool>, String> {
        Ok(self.0.lock().unwrap().blocked)
    }
    fn register(&self, id: i32, key: &Hotkey) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        state.attempts.push((id, key.canonical.clone()));
        if state.conflicts.contains(&key.canonical)
            || state
                .registered
                .values()
                .any(|registered| registered == key)
        {
            return Err("shortcut fixture conflict".into());
        }
        state.registered.insert(id, key.clone());
        Ok(())
    }
    fn unregister(&self, id: i32) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        state.unregistered.push(id);
        if state.fail_unregister.contains(&id) {
            return Err("shortcut fixture release failed".into());
        }
        state.registered.remove(&id);
        Ok(())
    }
}

struct Fixture {
    directory: PathBuf,
    path: PathBuf,
    executable: PathBuf,
    platform: MockPlatform,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("subgauge-desktop-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        Self {
            path: directory.join("desktop.v1.json"),
            executable: directory
                .join("Program Files")
                .join("中文")
                .join("subgauge.exe"),
            directory,
            platform: MockPlatform::default(),
        }
    }
    fn desktop(&self, distribution: Distribution) -> Desktop {
        Desktop::new(
            self.path.clone(),
            self.executable.clone(),
            distribution,
            Box::new(self.platform.clone()),
        )
        .unwrap()
    }
    fn installed(&self) -> Desktop {
        self.desktop(Distribution::Installed)
    }
    fn command(&self) -> String {
        startup_command(&self.executable).unwrap()
    }
    fn block_file_save(&self) {
        if self.path.is_file() {
            std::fs::remove_file(&self.path).unwrap();
        }
        std::fs::create_dir(&self.path).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn save(desktop: &Desktop, patch: DesktopPatch) -> Result<DesktopState, String> {
    desktop.save(SaveInput {
        expected_revision: desktop.status().revision,
        patch,
    })
}
fn shortcut(text: &str) -> DesktopPatch {
    DesktopPatch {
        shortcut: Some(Some(text.into())),
        ..Default::default()
    }
}

#[test]
fn defaults_and_repeated_status_reads_never_register_startup_or_shortcuts() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    desktop.restore_shortcut();
    for _ in 0..3 {
        let status = desktop.status();
        assert!(!status.launch_at_login);
        assert!(!status.shortcut_registered);
        assert_eq!(status.shortcut, None);
        assert_eq!(status.startup_visibility, StartupVisibility::Float);
        assert_eq!(status.startup_blocked, None);
    }
    let system = fixture.platform.0.lock().unwrap();
    assert!(system.reads > 0);
    assert!(system.writes.is_empty());
    assert!(system.attempts.is_empty());
    assert!(!fixture.path.exists());
}

#[test]
fn removing_startup_externally_is_respected_after_restart() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(desktop.status().launch_at_login);
    fixture.platform.0.lock().unwrap().run = None;
    let writes = fixture.platform.0.lock().unwrap().writes.len();
    let restarted = fixture.installed();
    restarted.restore_shortcut();
    assert!(!restarted.status().launch_at_login);
    assert_eq!(fixture.platform.0.lock().unwrap().writes.len(), writes);
}

#[test]
fn startup_source_and_visibility_do_not_hide_manual_launches() {
    assert!(from_autostart([
        "subgauge.exe".into(),
        "--autostart".into()
    ]));
    assert!(!from_autostart([
        "subgauge.exe".into(),
        "--autostart=false".into()
    ]));
    assert!(!from_autostart([
        "subgauge.exe".into(),
        "--AUTOSTART".into()
    ]));
    assert!(!from_autostart(Vec::<String>::new()));
    assert!(should_show(false, StartupVisibility::Float));
    assert!(should_show(false, StartupVisibility::Tray));
    assert!(should_show(true, StartupVisibility::Float));
    assert!(!should_show(true, StartupVisibility::Tray));
}

#[test]
fn startup_command_quotes_actual_absolute_path_and_rejects_unsafe_paths() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.command(),
        format!("\"{}\" --autostart", fixture.executable.display())
    );
    assert!(startup_command(Path::new("subgauge.exe")).is_err());
    for suffix in [
        "bad\"name.exe",
        "bad\nname.exe",
        "bad\rname.exe",
        "bad\0name.exe",
    ] {
        assert!(startup_command(&fixture.directory.join(suffix)).is_err());
    }
    assert!(startup_command(&fixture.directory.join("a".repeat(260))).is_err());
}

#[test]
fn long_executable_path_disables_startup_registration_without_preventing_other_settings() {
    let mut fixture = Fixture::new();
    fixture.executable = fixture.directory.join("a".repeat(260)).join("subgauge.exe");
    let desktop = fixture.installed();
    assert!(desktop.status().error.is_some());
    assert!(save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(true),
            ..Default::default()
        },
    )
    .is_err());
    let state = save(&desktop, shortcut("Ctrl+Alt+G")).unwrap();
    assert!(state.shortcut_registered);
    assert!(fixture.platform.0.lock().unwrap().writes.is_empty());
}

#[test]
fn corrupt_future_or_invalid_preferences_preserve_original_bytes_and_reject_saves() {
    for bytes in [
        b"{invalid".as_slice(),
        br#"{"version":2,"startupVisibility":"tray"}"#.as_slice(),
        br#"{"version":1,"extra":true}"#.as_slice(),
        br#"{"version":1,"shortcut":"Win+G"}"#.as_slice(),
    ] {
        let fixture = Fixture::new();
        std::fs::write(&fixture.path, bytes).unwrap();
        let desktop = fixture.installed();
        assert!(desktop.status().error.is_some());
        assert!(save(
            &desktop,
            DesktopPatch {
                launch_at_login: Some(true),
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(std::fs::read(&fixture.path).unwrap(), bytes);
        assert!(fixture.platform.0.lock().unwrap().writes.is_empty());
    }
}

#[test]
fn legacy_version_one_defaults_new_optional_fields_without_writing() {
    let fixture = Fixture::new();
    let bytes = br#"{"version":1}"#;
    std::fs::write(&fixture.path, bytes).unwrap();
    let desktop = fixture.installed();
    let status = desktop.status();
    assert!(status.error.is_none());
    assert_eq!(status.startup_visibility, StartupVisibility::Float);
    assert_eq!(status.shortcut, None);
    assert_eq!(std::fs::read(&fixture.path).unwrap(), bytes);
}

#[test]
fn shortcut_parser_normalizes_order_aliases_and_function_keys() {
    let key = parse_shortcut(" shift + ALT + control + g ").unwrap();
    assert_eq!(key.canonical, "Ctrl+Alt+Shift+G");
    assert_eq!(key.modifiers, 7);
    assert_eq!(key.key, u32::from(b'G'));
    assert_eq!(parse_shortcut("Alt+0").unwrap().canonical, "Alt+0");
    assert_eq!(parse_shortcut("Ctrl+f1").unwrap().key, 0x70);
    assert_eq!(parse_shortcut("Ctrl+F11").unwrap().key, 0x7a);
}

#[test]
fn shortcut_parser_rejects_reserved_ambiguous_or_unmodified_keys() {
    for text in [
        "",
        "G",
        "Shift+G",
        "Win+G",
        "Ctrl+F12",
        "Ctrl+F0",
        "Ctrl+F01",
        "Ctrl+Control+G",
        "Alt+Alt+G",
        "Ctrl++",
        "Ctrl+Space",
        "Ctrl+é",
        "Ctrl+GG",
    ] {
        assert!(
            parse_shortcut(text).is_err(),
            "unexpectedly accepted {text}"
        );
    }
    assert!(parse_shortcut(&format!("{}G", " ".repeat(49))).is_err());
}

#[test]
fn missing_null_and_present_shortcut_patches_are_distinct() {
    let untouched: DesktopPatch = serde_json::from_str("{}").unwrap();
    let cleared: DesktopPatch = serde_json::from_str(r#"{"shortcut":null}"#).unwrap();
    let set: DesktopPatch = serde_json::from_str(r#"{"shortcut":"Ctrl+Alt+G"}"#).unwrap();
    assert_eq!(untouched.shortcut, None);
    assert_eq!(cleared.shortcut, Some(None));
    assert_eq!(set.shortcut, Some(Some("Ctrl+Alt+G".into())));
    assert!(serde_json::from_str::<DesktopPatch>(r#"{"arbitrary":true}"#).is_err());
}

#[test]
fn shortcut_conflict_keeps_previous_key_and_saved_preferences() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    save(&desktop, shortcut("Ctrl+Alt+G")).unwrap();
    let original = std::fs::read(&fixture.path).unwrap();
    fixture
        .platform
        .0
        .lock()
        .unwrap()
        .conflicts
        .insert("Ctrl+Alt+H".into());
    assert!(save(&desktop, shortcut("Ctrl+Alt+H")).is_err());
    assert_eq!(desktop.status().shortcut.as_deref(), Some("Ctrl+Alt+G"));
    assert!(desktop.accepts_hotkey(HOTKEY_IDS[0]));
    assert!(!desktop.accepts_hotkey(HOTKEY_IDS[1]));
    assert_eq!(std::fs::read(&fixture.path).unwrap(), original);
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.registered.len(), 1);
    assert!(system.unregistered.is_empty());
}

#[test]
fn successful_key_change_accepts_only_new_id_and_clear_releases_it() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    save(&desktop, shortcut("Ctrl+Alt+G")).unwrap();
    save(&desktop, shortcut("Ctrl+Alt+H")).unwrap();
    assert!(!desktop.accepts_hotkey(HOTKEY_IDS[0]));
    assert!(desktop.accepts_hotkey(HOTKEY_IDS[1]));
    assert_eq!(fixture.platform.0.lock().unwrap().registered.len(), 1);
    let state = save(
        &desktop,
        DesktopPatch {
            shortcut: Some(None),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(state.shortcut, None);
    assert!(!state.shortcut_registered);
    assert!(!desktop.accepts_hotkey(HOTKEY_IDS[1]));
    assert!(fixture.platform.0.lock().unwrap().registered.is_empty());
}

#[test]
fn equivalent_shortcut_spelling_does_not_reregister_or_conflict_with_itself() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    save(&desktop, shortcut("Ctrl+Alt+G")).unwrap();
    save(&desktop, shortcut("alt + control + g")).unwrap();
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.attempts.len(), 1);
    assert!(system.unregistered.is_empty());
}

#[test]
fn file_save_failure_restores_run_and_releases_candidate_without_losing_old_key() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    save(&desktop, shortcut("Ctrl+Alt+G")).unwrap();
    fixture.block_file_save();
    assert!(save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(true),
            shortcut: Some(Some("Ctrl+Alt+H".into())),
            startup_visibility: Some(StartupVisibility::Tray),
        }
    )
    .is_err());
    let status = desktop.status();
    assert!(!status.launch_at_login);
    assert_eq!(status.startup_visibility, StartupVisibility::Float);
    assert_eq!(status.shortcut.as_deref(), Some("Ctrl+Alt+G"));
    assert!(desktop.accepts_hotkey(HOTKEY_IDS[0]));
    assert!(!desktop.accepts_hotkey(HOTKEY_IDS[1]));
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.run, None);
    assert_eq!(system.writes, vec![Some(fixture.command()), None]);
    assert_eq!(system.registered.len(), 1);
    assert_eq!(system.unregistered, vec![HOTKEY_IDS[1]]);
}

#[test]
fn registry_failure_releases_candidate_before_any_preference_is_persisted() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    fixture.platform.0.lock().unwrap().fail_write = true;
    assert!(save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(true),
            shortcut: Some(Some("Ctrl+Alt+G".into())),
            ..Default::default()
        }
    )
    .is_err());
    assert!(!fixture.path.exists());
    assert_eq!(desktop.status().shortcut, None);
    assert!(fixture.platform.0.lock().unwrap().registered.is_empty());
}

#[test]
fn failed_disable_transaction_restores_previously_enabled_startup() {
    let fixture = Fixture::new();
    fixture.platform.0.lock().unwrap().run = Some(fixture.command());
    let desktop = fixture.installed();
    fixture.block_file_save();
    assert!(save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(false),
            startup_visibility: Some(StartupVisibility::Tray),
            ..Default::default()
        },
    )
    .is_err());
    assert!(desktop.status().launch_at_login);
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.run, Some(fixture.command()));
    assert_eq!(system.writes, vec![None, Some(fixture.command())]);
}

#[test]
fn candidate_release_failure_keeps_candidate_inactive_and_requires_restart() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    save(&desktop, shortcut("Ctrl+Alt+G")).unwrap();
    fixture.block_file_save();
    fixture
        .platform
        .0
        .lock()
        .unwrap()
        .fail_unregister
        .insert(HOTKEY_IDS[1]);
    assert!(save(&desktop, shortcut("Ctrl+Alt+H")).is_err());
    assert!(desktop.accepts_hotkey(HOTKEY_IDS[0]));
    assert!(!desktop.accepts_hotkey(HOTKEY_IDS[1]));
    assert!(desktop.status().error.is_some());
    assert!(save(&desktop, shortcut("Ctrl+Alt+J")).is_err());
}

#[test]
fn rollback_preserves_external_run_change_instead_of_overwriting_it() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    fixture.block_file_save();
    let external = r#""D:\Other\subgauge.exe" --autostart"#.to_string();
    fixture.platform.0.lock().unwrap().external_after_write = Some(external.clone());
    let result = save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(true),
            startup_visibility: Some(StartupVisibility::Tray),
            ..Default::default()
        },
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("fixture should fail saving to a directory"),
    };
    assert!(error.contains("外部"));
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.run, Some(external));
    assert_eq!(system.writes.len(), 1);
}

#[test]
fn external_run_change_before_write_releases_candidate_and_never_overwrites_it() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    let revision = desktop.status().revision;
    let external = r#""D:\Other\subgauge.exe" --autostart"#.to_string();
    {
        let mut system = fixture.platform.0.lock().unwrap();
        system.external_on_read = Some((system.reads + 3, external.clone()));
    }
    assert!(desktop
        .save(SaveInput {
            expected_revision: revision,
            patch: DesktopPatch {
                launch_at_login: Some(true),
                shortcut: Some(Some("Ctrl+Alt+G".into())),
                ..Default::default()
            },
        })
        .is_err());
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.run, Some(external));
    assert!(system.writes.is_empty());
    assert!(system.registered.is_empty());
    assert!(!fixture.path.exists());
}

#[test]
fn external_run_change_after_write_rejects_even_when_preferences_file_is_writable() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    let external = r#""D:\Other\subgauge.exe" --autostart"#.to_string();
    fixture.platform.0.lock().unwrap().external_after_write = Some(external.clone());
    assert!(save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(true),
            startup_visibility: Some(StartupVisibility::Tray),
            shortcut: Some(Some("Ctrl+Alt+G".into())),
        }
    )
    .is_err());
    assert!(!fixture.path.exists());
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.run, Some(external));
    assert_eq!(system.writes.len(), 1);
    assert!(system.registered.is_empty());
}

#[test]
fn external_run_or_approval_change_rejects_stale_revision_without_writing() {
    for change_run in [true, false] {
        let fixture = Fixture::new();
        let desktop = fixture.installed();
        let revision = desktop.status().revision;
        if change_run {
            fixture.platform.0.lock().unwrap().run = Some(fixture.command());
        } else {
            fixture.platform.0.lock().unwrap().blocked = Some(true);
        }
        assert!(desktop
            .save(SaveInput {
                expected_revision: revision,
                patch: DesktopPatch {
                    startup_visibility: Some(StartupVisibility::Tray),
                    ..Default::default()
                },
            })
            .is_err());
        assert!(desktop.status().revision > revision);
        assert!(fixture.platform.0.lock().unwrap().writes.is_empty());
        assert!(!fixture.path.exists());
    }
}

#[test]
fn portable_cannot_enable_or_remove_installers_startup_but_can_use_shortcut() {
    let fixture = Fixture::new();
    let existing = r#""D:\Installed\subgauge.exe" --autostart"#.to_string();
    fixture.platform.0.lock().unwrap().run = Some(existing.clone());
    let desktop = fixture.desktop(Distribution::Portable);
    for enabled in [true, false] {
        assert!(save(
            &desktop,
            DesktopPatch {
                launch_at_login: Some(enabled),
                ..Default::default()
            }
        )
        .is_err());
    }
    assert!(
        save(&desktop, shortcut("Ctrl+Alt+G"))
            .unwrap()
            .shortcut_registered
    );
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.run, Some(existing));
    assert!(system.writes.is_empty());
    assert_eq!(system.reads, 0);
}

#[test]
fn development_mode_does_not_claim_global_keys_or_register_startup() {
    let fixture = Fixture::new();
    let desktop = fixture.desktop(Distribution::Development);
    assert!(save(&desktop, shortcut("Ctrl+Alt+G")).is_err());
    assert!(save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(true),
            ..Default::default()
        }
    )
    .is_err());
    assert!(fixture.platform.0.lock().unwrap().attempts.is_empty());
    assert!(fixture.platform.0.lock().unwrap().writes.is_empty());
}

#[test]
fn blocked_startup_is_reported_without_changing_windows_approval() {
    let fixture = Fixture::new();
    {
        let mut system = fixture.platform.0.lock().unwrap();
        system.run = Some(fixture.command());
        system.blocked = Some(true);
    }
    let desktop = fixture.installed();
    desktop.restore_shortcut();
    let state = save(
        &desktop,
        DesktopPatch {
            startup_visibility: Some(StartupVisibility::Tray),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(state.launch_at_login);
    assert_eq!(state.startup_blocked, Some(true));
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.blocked, Some(true));
    assert!(system.writes.is_empty());
}

#[test]
fn another_directory_startup_is_neither_overwritten_nor_deleted() {
    let fixture = Fixture::new();
    let other = r#""D:\Other\subgauge.exe" --autostart"#.to_string();
    fixture.platform.0.lock().unwrap().run = Some(other.clone());
    let desktop = fixture.installed();
    assert!(desktop.status().error.is_some());
    for enabled in [true, false] {
        assert!(save(
            &desktop,
            DesktopPatch {
                launch_at_login: Some(enabled),
                ..Default::default()
            }
        )
        .is_err());
    }
    save(
        &desktop,
        DesktopPatch {
            startup_visibility: Some(StartupVisibility::Tray),
            ..Default::default()
        },
    )
    .unwrap();
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.run, Some(other));
    assert!(system.writes.is_empty());
}

#[test]
fn restart_shortcut_conflict_reports_unavailable_and_preserves_preference() {
    let fixture = Fixture::new();
    std::fs::write(&fixture.path, br#"{"version":1,"shortcut":"Ctrl+Alt+G"}"#).unwrap();
    fixture
        .platform
        .0
        .lock()
        .unwrap()
        .conflicts
        .insert("Ctrl+Alt+G".into());
    let desktop = fixture.installed();
    desktop.restore_shortcut();
    let status = desktop.status();
    assert_eq!(status.shortcut.as_deref(), Some("Ctrl+Alt+G"));
    assert!(!status.shortcut_registered);
    assert!(status.error.is_some());
    assert!(fixture.platform.0.lock().unwrap().writes.is_empty());
}

#[test]
fn restarting_with_conflicted_shortcut_still_allows_unrelated_startup_changes() {
    let fixture = Fixture::new();
    std::fs::write(&fixture.path, br#"{"version":1,"shortcut":"Ctrl+Alt+G"}"#).unwrap();
    fixture
        .platform
        .0
        .lock()
        .unwrap()
        .conflicts
        .insert("Ctrl+Alt+G".into());
    let desktop = fixture.installed();
    desktop.restore_shortcut();
    let attempts_before = fixture.platform.0.lock().unwrap().attempts.len();
    let state = save(
        &desktop,
        DesktopPatch {
            launch_at_login: Some(true),
            startup_visibility: Some(StartupVisibility::Tray),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(state.launch_at_login);
    assert_eq!(state.startup_visibility, StartupVisibility::Tray);
    assert_eq!(state.shortcut.as_deref(), Some("Ctrl+Alt+G"));
    assert!(!state.shortcut_registered);
    assert!(state.error.is_some());
    let system = fixture.platform.0.lock().unwrap();
    assert_eq!(system.attempts.len(), attempts_before);
    assert_eq!(system.run, Some(fixture.command()));
    assert_eq!(system.writes.len(), 1);
}

#[test]
fn old_key_release_failure_filters_queued_old_events_and_blocks_further_changes() {
    let fixture = Fixture::new();
    let desktop = fixture.installed();
    save(&desktop, shortcut("Ctrl+Alt+G")).unwrap();
    fixture
        .platform
        .0
        .lock()
        .unwrap()
        .fail_unregister
        .insert(HOTKEY_IDS[0]);
    let state = save(&desktop, shortcut("Ctrl+Alt+H")).unwrap();
    assert!(state.error.is_some());
    assert!(!desktop.accepts_hotkey(HOTKEY_IDS[0]));
    assert!(desktop.accepts_hotkey(HOTKEY_IDS[1]));
    assert!(save(&desktop, shortcut("Ctrl+Alt+J")).is_err());
    assert_eq!(fixture.platform.0.lock().unwrap().registered.len(), 2);
}
