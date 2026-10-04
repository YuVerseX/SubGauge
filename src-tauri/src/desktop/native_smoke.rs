#![cfg(windows)]

use super::*;
use std::sync::Arc;
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT},
    WindowsAndMessaging::{IsWindow, IsWindowVisible, SendMessageW, WM_HOTKEY},
};

const PROBE_IDS: [i32; 2] = [0x5a10, 0x5a11];

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "subgauge-desktop-native-smoke-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the unique subtree created by this test can be removed.
        if self.0.parent() == Some(std::env::temp_dir().as_path())
            && self.0.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("subgauge-desktop-native-smoke-")
            })
        {
            // WebView2 may release its profile shortly after HWND destruction.
            for attempt in 0..20 {
                match std::fs::remove_dir_all(&self.0) {
                    Ok(()) => return,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                    Err(error) if attempt == 19 => {
                        eprintln!(
                            "Isolated WebView fixture cleanup incomplete at {}: {error}",
                            self.0.display()
                        );
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
                }
            }
        }
    }
}

struct Opponent {
    hwnd: usize,
    registered: Vec<i32>,
}
impl Opponent {
    fn new(app: &AppHandle) -> Result<Self, String> {
        let window = app
            .get_webview_window("opponent")
            .ok_or("Opponent HWND missing")?;
        Ok(Self {
            hwnd: window.hwnd().map_err(|e| e.to_string())?.0 as usize,
            registered: vec![],
        })
    }
    fn register(&mut self, id: i32, key: &Hotkey) -> bool {
        let registered = unsafe {
            RegisterHotKey(self.hwnd as _, id, key.modifiers | MOD_NOREPEAT, key.key) != 0
        };
        if registered {
            self.registered.push(id);
        }
        registered
    }
    fn release(&mut self, id: i32) -> Result<(), String> {
        if unsafe { UnregisterHotKey(self.hwnd as _, id) } == 0 {
            return Err("Opponent hotkey release failed".into());
        }
        self.registered.retain(|registered| *registered != id);
        Ok(())
    }
}
impl Drop for Opponent {
    fn drop(&mut self) {
        for id in &self.registered {
            let _ = unsafe { UnregisterHotKey(self.hwnd as _, *id) };
        }
    }
}
struct SystemRegistrations(native::System);
impl Drop for SystemRegistrations {
    fn drop(&mut self) {
        for id in PROBE_IDS {
            let _ = self.0.unregister(id);
        }
    }
}

fn check(checks: &mut Vec<&'static str>, name: &'static str, passed: bool) -> Result<(), String> {
    if !passed {
        return Err(name.into());
    }
    checks.push(name);
    Ok(())
}

fn initial_checks(
    app: &AppHandle,
    directory: PathBuf,
) -> Result<(usize, Vec<&'static str>), String> {
    let mut checks = vec![];
    let first = parse_shortcut("Ctrl+Alt+Shift+F10")?;
    let second = parse_shortcut("Ctrl+Alt+Shift+F11")?;
    let native = SystemRegistrations(native::System::new(app)?);
    let mut opponent = Opponent::new(app)?;
    native.0.register(PROBE_IDS[0], &first)?;
    check(
        &mut checks,
        "same-combination-on-second-hwnd-is-rejected",
        !opponent.register(PROBE_IDS[0], &first),
    )?;
    native.0.register(PROBE_IDS[1], &second)?;
    check(
        &mut checks,
        "candidate-registers-before-old-combination-released",
        !opponent.register(PROBE_IDS[0], &first) && !opponent.register(PROBE_IDS[1], &second),
    )?;
    native.0.unregister(PROBE_IDS[0])?;
    check(
        &mut checks,
        "released-old-combination-can-be-registered-by-second-hwnd",
        opponent.register(PROBE_IDS[0], &first),
    )?;
    opponent.release(PROBE_IDS[0])?;
    native.0.unregister(PROBE_IDS[1])?;
    check(
        &mut checks,
        "explicitly-released-candidate-can-be-registered-by-second-hwnd",
        opponent.register(PROBE_IDS[1], &second),
    )?;
    opponent.release(PROBE_IDS[1])?;
    drop(native);

    // Portable skips all Run and StartupApproved reads/writes, including status().
    let path = directory.join("desktop.v1.json");
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let original = Desktop::new(
        path.clone(),
        executable.clone(),
        Distribution::Portable,
        Box::new(native::System::new(app)?),
    )?;
    original.restore_shortcut();
    let saved = original.save(SaveInput {
        expected_revision: original.status().revision,
        patch: DesktopPatch {
            startup_visibility: Some(StartupVisibility::Tray),
            shortcut: Some(Some(first.canonical.clone())),
            ..Default::default()
        },
    })?;
    check(
        &mut checks,
        "portable-preference-save-registers-shortcut-without-launch-at-login",
        saved.shortcut_registered
            && !saved.launch_at_login
            && saved.startup_visibility == StartupVisibility::Tray,
    )?;
    let persisted = std::fs::read(&path).map_err(|e| e.to_string())?;
    let preferences: Preferences = serde_json::from_slice(&persisted).map_err(|e| e.to_string())?;
    check(
        &mut checks,
        "fixture-persists-shortcut-and-startup-visibility",
        preferences.shortcut.as_deref() == Some(first.canonical.as_str())
            && preferences.startup_visibility == StartupVisibility::Tray,
    )?;
    original.release();
    let restored = Desktop::new(
        path,
        executable,
        Distribution::Portable,
        Box::new(native::System::new(app)?),
    )?;
    restored.restore_shortcut();
    check(
        &mut checks,
        "new-service-restores-persisted-shortcut-registration",
        restored.status().shortcut_registered && restored.accepts_hotkey(HOTKEY_IDS[0]),
    )?;
    let replaced = restored.save(SaveInput {
        expected_revision: restored.status().revision,
        patch: DesktopPatch {
            shortcut: Some(Some(second.canonical.clone())),
            ..Default::default()
        },
    })?;
    check(
        &mut checks,
        "native-service-candidate-change-activates-new-id-only",
        replaced.shortcut_registered
            && restored.accepts_hotkey(HOTKEY_IDS[1])
            && !restored.accepts_hotkey(HOTKEY_IDS[0]),
    )?;
    check(
        &mut checks,
        "native-service-replacement-releases-old-key-and-retains-new-key",
        opponent.register(PROBE_IDS[0], &first) && !opponent.register(PROBE_IDS[1], &second),
    )?;
    opponent.release(PROBE_IDS[0])?;
    app.manage(restored);
    app.state::<crate::windows::Windows>()
        .install_native_hook(app)?;
    let float = app.get_webview_window("float").ok_or("Float missing")?;
    let hwnd = float.hwnd().map_err(|e| e.to_string())?.0 as usize;
    // An inactive ID exercises the real hook without calling show/hide/recovery.
    unsafe { SendMessageW(hwnd as _, WM_HOTKEY, 0x5aff, 0) };
    check(
        &mut checks,
        "inactive-hotkey-message-keeps-both-windows-hidden",
        unsafe { IsWindowVisible(hwnd as _) == 0 && IsWindowVisible(opponent.hwnd as _) == 0 },
    )?;
    float.destroy().map_err(|e| e.to_string())?;
    Ok((hwnd, checks))
}

#[test]
#[ignore = "requires Windows WebView2; isolated hidden HWNDs, synthetic local preferences, no real keys or startup registry"]
fn native_desktop_smoke() {
    let fixture = Fixture::new();
    let directory = fixture.0.clone();
    let outcome = Arc::new(Mutex::new(None::<Result<Vec<&'static str>, String>>));
    let target = outcome.clone();
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = format!(
        "app.subgauge.desktop-smoke-{}",
        uuid::Uuid::new_v4().simple()
    );
    let app = tauri::Builder::default()
        .any_thread()
        .setup(move |app| {
            app.manage(crate::windows::Windows::new(directory.join("window.json")));
            for label in ["float", "opponent"] {
                tauri::WebviewWindowBuilder::new(
                    app,
                    label,
                    tauri::WebviewUrl::External("about:blank".parse().unwrap()),
                )
                .visible(false)
                .skip_taskbar(true)
                .data_directory(directory.join("webview"))
                .build()?;
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    let (hwnd, mut checks) =
                        on_main(handle.clone(), move |app| initial_checks(app, directory)).await?;
                    for _ in 0..100 {
                        let finished = on_main(handle.clone(), move |_| {
                            Ok(unsafe { IsWindow(hwnd as _) == 0 })
                        })
                        .await?;
                        if finished {
                            let mut cleanup_checks = on_main(handle.clone(), |app| {
                                let desktop = app.state::<Desktop>();
                                let mut checks = vec![];
                                check(
                                    &mut checks,
                                    "wm-ncdestroy-releases-managed-shortcut-state",
                                    !desktop.status().shortcut_registered
                                        && !desktop.accepts_hotkey(HOTKEY_IDS[1]),
                                )?;
                                let mut opponent = Opponent::new(app)?;
                                check(
                                    &mut checks,
                                    "destroyed-float-shortcut-can-be-registered-by-surviving-hwnd",
                                    opponent.register(
                                        PROBE_IDS[1],
                                        &parse_shortcut("Ctrl+Alt+Shift+F11")?,
                                    ),
                                )?;
                                opponent.release(PROBE_IDS[1])?;
                                check(&mut checks, "surviving-hwnd-remains-hidden", unsafe {
                                    IsWindowVisible(opponent.hwnd as _) == 0
                                })?;
                                Ok(checks)
                            })
                            .await?;
                            checks.append(&mut cleanup_checks);
                            return Ok(checks);
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    Err("Hidden float HWND did not finish destruction".into())
                }
                .await;
                *target.lock().unwrap() = Some(result);
                let cleanup_handle = handle.clone();
                let _ = on_main(handle, move |app| {
                    if let Some(desktop) = app.try_state::<Desktop>() {
                        desktop.release();
                    }
                    for label in ["float", "opponent"] {
                        if let Some(window) = app.get_webview_window(label) {
                            let _ = window.destroy();
                        }
                    }
                    cleanup_handle.exit(0);
                    Ok(())
                })
                .await;
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    let exit = app.run_return(|_, _| {});
    assert_eq!(exit, 0);
    let checks = outcome
        .lock()
        .unwrap()
        .take()
        .expect("Native desktop smoke did not finish")
        .unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "scope":"Hidden native WebView2 HWNDs and real RegisterHotKey; Portable fixture preferences; no startup registry, physical keyboard, mouse, visible app, or actual accounts; hotkey-driven UI behavior is not validated",
            "passed":true,
            "checks":checks
        }))
        .unwrap()
    );
}
