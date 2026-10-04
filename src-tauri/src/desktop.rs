mod native;
#[cfg(all(test, windows))]
#[path = "desktop/native_smoke.rs"]
mod native_smoke;
#[cfg(test)]
mod tests;

use crate::{core::atomic_write, updates::Distribution};
use serde::{Deserialize, Deserializer, Serialize};
use std::{path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub(crate) const STARTUP_VALUE: &str = "SubGauge.Desktop";
const HOTKEY_IDS: [i32; 2] = [0x5350, 0x5351];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StartupVisibility {
    #[default]
    Float,
    Tray,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    version: u32,
    #[serde(default)]
    startup_visibility: StartupVisibility,
    #[serde(default)]
    shortcut: Option<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            startup_visibility: StartupVisibility::Float,
            shortcut: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopState {
    pub revision: u64,
    pub distribution: Distribution,
    /// Registration for this executable, not a promise that Windows permits it.
    pub launch_at_login: bool,
    pub startup_visibility: StartupVisibility,
    pub shortcut: Option<String>,
    pub shortcut_registered: bool,
    pub startup_blocked: Option<bool>,
    pub error: Option<String>,
}
fn nullable<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopPatch {
    pub launch_at_login: Option<bool>,
    pub startup_visibility: Option<StartupVisibility>,
    #[serde(default, deserialize_with = "nullable")]
    pub shortcut: Option<Option<String>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveInput {
    pub expected_revision: u64,
    pub patch: DesktopPatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hotkey {
    pub canonical: String,
    pub modifiers: u32,
    pub key: u32,
}
fn parse_shortcut(text: &str) -> Result<Hotkey, String> {
    if text.len() > 48 {
        return Err("快捷键格式无效。".into());
    }
    let parts: Vec<_> = text.split('+').map(str::trim).collect();
    let (last, modifiers) = parts.split_last().ok_or("请输入快捷键。")?;
    let mut mask = 0;
    for part in modifiers {
        let bit = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => 2,
            "alt" => 1,
            "shift" => 4,
            _ => return Err("仅支持 Ctrl、Alt、Shift 加字母、数字或 F1～F11。".into()),
        };
        if mask & bit != 0 {
            return Err("快捷键不能重复修饰键。".into());
        }
        mask |= bit;
    }
    if mask & 3 == 0 {
        return Err("快捷键必须至少包含 Ctrl 或 Alt。".into());
    }
    let key_text = last.to_ascii_uppercase();
    let key = if key_text.len() == 1 && key_text.as_bytes()[0].is_ascii_alphanumeric() {
        u32::from(key_text.as_bytes()[0])
    } else if let Some(number) = key_text
        .strip_prefix('F')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (1..=11).contains(n))
    {
        if key_text != format!("F{number}") {
            return Err("功能键格式无效。".into());
        }
        0x70 + number - 1
    } else {
        return Err("仅支持字母、数字或 F1～F11，不支持 Windows 键和 F12。".into());
    };
    let mut canonical = Vec::new();
    if mask & 2 != 0 {
        canonical.push("Ctrl");
    }
    if mask & 1 != 0 {
        canonical.push("Alt");
    }
    if mask & 4 != 0 {
        canonical.push("Shift");
    }
    canonical.push(&key_text);
    Ok(Hotkey {
        canonical: canonical.join("+"),
        modifiers: mask,
        key,
    })
}

pub(crate) trait Platform: Send + Sync {
    fn read_run(&self) -> Result<Option<String>, String>;
    fn write_run(&self, value: Option<&str>) -> Result<(), String>;
    fn startup_blocked(&self) -> Result<Option<bool>, String>;
    fn register(&self, id: i32, key: &Hotkey) -> Result<(), String>;
    fn unregister(&self, id: i32) -> Result<(), String>;
}
struct Inner {
    preferences: Preferences,
    state: DesktopState,
    load_error: Option<String>,
    shortcut_error: Option<String>,
    active: Option<(i32, Hotkey)>,
    cleanup_failed: bool,
}
pub struct Desktop {
    path: PathBuf,
    command: String,
    command_error: Option<String>,
    platform: Box<dyn Platform>,
    inner: Mutex<Inner>,
}
fn startup_command(path: &std::path::Path) -> Result<String, String> {
    let text = path
        .to_str()
        .filter(|_| path.is_absolute())
        .ok_or("启动程序路径无效。")?;
    if text.contains(['"', '\0', '\r', '\n']) {
        return Err("启动程序路径无效。".into());
    }
    let command = format!("\"{text}\" --autostart");
    if command.encode_utf16().count() > 260 {
        return Err("程序路径过长，无法登记登录启动。".into());
    }
    Ok(command)
}
pub(crate) fn from_autostart(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|arg| arg == "--autostart")
}
pub(crate) fn should_show(autostart: bool, visibility: StartupVisibility) -> bool {
    !autostart || visibility == StartupVisibility::Float
}
impl Desktop {
    fn new(
        path: PathBuf,
        executable: PathBuf,
        distribution: Distribution,
        platform: Box<dyn Platform>,
    ) -> Result<Self, String> {
        let (preferences, load_error) = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(p) if p.version == 1 && p.shortcut.as_deref().is_none_or(|s| parse_shortcut(s).is_ok()) => (p, None),
                _ => (Preferences::default(), Some("启动设置损坏或来自新版，原文件已保留；请备份后移开该文件并重启，再保存设置。".into())),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Preferences::default(), None),
            Err(_) => (Preferences::default(), Some("启动设置无法读取，原文件已保留。".into())),
        };
        let state = DesktopState {
            revision: 1,
            distribution,
            launch_at_login: false,
            startup_visibility: preferences.startup_visibility,
            shortcut: preferences.shortcut.clone(),
            shortcut_registered: false,
            startup_blocked: None,
            error: load_error.clone(),
        };
        let command_result = startup_command(&executable);
        let command_error = command_result.as_ref().err().cloned();
        Ok(Self {
            path,
            command: command_result.unwrap_or_default(),
            command_error,
            platform,
            inner: Mutex::new(Inner {
                preferences,
                state,
                load_error,
                shortcut_error: None,
                active: None,
                cleanup_failed: false,
            }),
        })
    }
    fn refresh_system(&self, inner: &mut Inner) {
        let old = (
            inner.state.launch_at_login,
            inner.state.startup_blocked,
            inner.state.error.clone(),
        );
        let mut error = inner
            .load_error
            .clone()
            .or_else(|| inner.shortcut_error.clone());
        inner.state.launch_at_login = false;
        inner.state.startup_blocked = None;
        if inner.state.distribution == Distribution::Installed {
            if let Some(e) = &self.command_error {
                error = Some(e.clone());
            }
            match self.platform.read_run() {
                Ok(Some(value)) if value.eq_ignore_ascii_case(&self.command) => {
                    inner.state.launch_at_login = true
                }
                Ok(Some(_)) => error = Some(
                    "另一个目录已有 SubGauge 自启登记，请先在那个安装目录关闭；本次不会覆盖它。"
                        .into(),
                ),
                Ok(None) => {}
                Err(e) => error = Some(e),
            }
            match self.platform.startup_blocked() {
                Ok(value) => inner.state.startup_blocked = value,
                Err(e) => error = Some(e),
            }
        }
        inner.state.error = error;
        if old
            != (
                inner.state.launch_at_login,
                inner.state.startup_blocked,
                inner.state.error.clone(),
            )
        {
            inner.state.revision += 1;
        }
    }
    pub fn status(&self) -> DesktopState {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.refresh_system(&mut inner);
        inner.state.clone()
    }
    /// Invoked once on the HWND-owning thread, never reenables a login entry.
    fn restore_shortcut(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.state.distribution != Distribution::Development {
            if let Some(key) = inner
                .preferences
                .shortcut
                .as_deref()
                .and_then(|s| parse_shortcut(s).ok())
            {
                match self.platform.register(HOTKEY_IDS[0], &key) {
                    Ok(()) => {
                        inner.active = Some((HOTKEY_IDS[0], key));
                        inner.state.shortcut_registered = true;
                    }
                    Err(e) => inner.shortcut_error = Some(e),
                }
            }
        }
        self.refresh_system(&mut inner);
    }
    fn save(&self, input: SaveInput) -> Result<DesktopState, String> {
        let mut inner = self.inner.lock().map_err(|_| "启动设置状态不可用。")?;
        self.refresh_system(&mut inner);
        if input.expected_revision != inner.state.revision {
            return Err("启动设置或系统登记已变化，请重新载入后保存。".into());
        }
        if let Some(e) = &inner.load_error {
            return Err(e.clone());
        }
        if inner.cleanup_failed {
            return Err("旧快捷键释放失败，请重启应用后再修改。".into());
        }
        let mut next = inner.preferences.clone();
        let shortcut_requested = input.patch.shortcut.is_some();
        if let Some(visibility) = input.patch.startup_visibility {
            next.startup_visibility = visibility;
        }
        if let Some(shortcut) = input.patch.shortcut {
            next.shortcut = shortcut
                .map(|s| parse_shortcut(&s).map(|key| key.canonical))
                .transpose()?;
        }
        let key = next.shortcut.as_deref().map(parse_shortcut).transpose()?;
        if shortcut_requested
            && inner.state.distribution == Distribution::Development
            && key.is_some()
        {
            return Err("开发模式不注册全局快捷键，请使用发行版。".into());
        }
        let old_run = if input.patch.launch_at_login.is_some() {
            if inner.state.distribution != Distribution::Installed {
                return Err("开机自启仅支持安装版；免安装版不会修改安装版的启动项。".into());
            }
            if let Some(e) = &self.command_error {
                return Err(e.clone());
            }
            let value = self.platform.read_run()?;
            if value
                .as_ref()
                .is_some_and(|v| !v.eq_ignore_ascii_case(&self.command))
            {
                return Err("已有其他目录的启动项，本次不会覆盖或删除。".into());
            }
            Some(value)
        } else {
            None
        };
        let desired_run = input
            .patch
            .launch_at_login
            .map(|enabled| enabled.then(|| self.command.clone()));
        let active_key = inner.active.as_ref().map(|(_, k)| k);
        let key_changed = shortcut_requested && key.as_ref() != active_key;
        let candidate = if key_changed {
            key.as_ref().map(|k| {
                (
                    if inner
                        .active
                        .as_ref()
                        .is_some_and(|(id, _)| *id == HOTKEY_IDS[0])
                    {
                        HOTKEY_IDS[1]
                    } else {
                        HOTKEY_IDS[0]
                    },
                    k.clone(),
                )
            })
        } else {
            None
        };
        if let Some((id, key)) = &candidate {
            self.platform.register(*id, key)?;
        }
        let run_changed = old_run
            .as_ref()
            .zip(desired_run.as_ref())
            .is_some_and(|(old, new)| old != new);
        let mut run_written = false;
        let result = (|| {
            if run_changed {
                if old_run.as_ref() != Some(&self.platform.read_run()?) {
                    return Err("启动项已被外部修改，请重新载入后保存。".into());
                }
                self.platform
                    .write_run(desired_run.as_ref().and_then(|v| v.as_deref()))?;
                run_written = true;
                if desired_run.as_ref() != Some(&self.platform.read_run()?) {
                    return Err("启动项保存后已被外部修改，请检查 Windows 启动应用。".into());
                }
            }
            if next != inner.preferences {
                let bytes = serde_json::to_vec_pretty(&next).map_err(|_| "启动设置格式无效。")?;
                atomic_write(&self.path, &bytes)?;
            }
            Ok::<(), String>(())
        })();
        if let Err(mut error) = result {
            if run_written {
                // Only restore our own staged value; never replace an external change.
                match self.platform.read_run() {
                    Ok(current) if desired_run.as_ref() == Some(&current) => {
                        if self
                            .platform
                            .write_run(old_run.as_ref().and_then(|v| v.as_deref()))
                            .is_err()
                        {
                            error.push_str(" 启动项恢复失败，请检查 Windows 启动应用。");
                        }
                    }
                    _ => error.push_str(" 启动项已被外部修改，未覆盖外部状态。"),
                }
            }
            if let Some((id, _)) = candidate {
                if self.platform.unregister(id).is_err() {
                    inner.cleanup_failed = true;
                    inner.shortcut_error = Some("快捷键释放失败，请重启应用。".into());
                }
            }
            self.refresh_system(&mut inner);
            inner.state.revision += 1;
            return Err(error);
        }
        if key_changed {
            let old = inner.active.take();
            inner.active = candidate;
            if let Some((id, _)) = old {
                if let Err(e) = self.platform.unregister(id) {
                    inner.cleanup_failed = true;
                    inner.shortcut_error = Some(e);
                }
            }
        }
        inner.preferences = next;
        inner.state.startup_visibility = inner.preferences.startup_visibility;
        inner.state.shortcut = inner.preferences.shortcut.clone();
        inner.state.shortcut_registered = inner.active.is_some();
        if shortcut_requested && !inner.cleanup_failed {
            inner.shortcut_error = None;
        }
        inner.state.revision += 1;
        self.refresh_system(&mut inner);
        Ok(inner.state.clone())
    }
    pub(crate) fn accepts_hotkey(&self, id: i32) -> bool {
        self.inner.lock().is_ok_and(|inner| {
            inner
                .active
                .as_ref()
                .is_some_and(|(active, _)| *active == id)
        })
    }
    pub(crate) fn release(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            for id in HOTKEY_IDS {
                let _ = self.platform.unregister(id);
            }
            inner.active = None;
            inner.state.shortcut_registered = false;
        }
    }
}

pub(crate) fn initialize(
    app: &AppHandle,
    path: PathBuf,
    executable: PathBuf,
    distribution: Distribution,
) -> Result<StartupVisibility, String> {
    let desktop = Desktop::new(
        path,
        executable,
        distribution,
        Box::new(native::System::new(app)?),
    )?;
    desktop.restore_shortcut();
    let visibility = desktop.status().startup_visibility;
    app.manage(desktop);
    Ok(visibility)
}
async fn on_main<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, recv) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = send.send(work(&handle));
    })
    .map_err(|_| "无法访问桌面设置。")?;
    recv.await.map_err(|_| "桌面设置操作中断。")?
}
#[tauri::command]
pub async fn desktop_status(app: AppHandle) -> Result<DesktopState, String> {
    on_main(app, |app| {
        let state = app.state::<Desktop>().status();
        let _ = app.emit("subgauge:desktop-state", &state);
        Ok(state)
    })
    .await
}
#[tauri::command]
pub async fn save_desktop_preferences(
    app: AppHandle,
    input: SaveInput,
) -> Result<DesktopState, String> {
    let _activity = app.state::<crate::updates::Updates>().activity().await?;
    on_main(app, move |app| {
        let desktop = app.state::<Desktop>();
        let result = desktop.save(input);
        let _ = app.emit("subgauge:desktop-state", desktop.status());
        result
    })
    .await
}
#[tauri::command]
pub fn open_startup_settings() -> Result<(), String> {
    native::open_startup_settings()
}

pub(crate) fn hotkey(app: &AppHandle, id: i32) {
    if app
        .try_state::<Desktop>()
        .is_none_or(|d| !d.accepts_hotkey(id))
        || app.state::<crate::updates::Updates>().is_installing()
    {
        return;
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if handle.state::<crate::updates::Updates>().is_installing()
            || !handle.state::<Desktop>().accepts_hotkey(id)
        {
            return;
        }
        let Some(window) = handle.get_webview_window("float") else {
            return;
        };
        let result = if window.is_visible().unwrap_or(false) {
            window.hide().map_err(|_| "无法隐藏浮窗。".into())
        } else {
            crate::windows::show_float_impl(&handle)
        };
        if let Err(error) = result {
            let _ = handle.emit("subgauge:settings-error", error);
        }
    });
}
