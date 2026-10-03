use base64::Engine as _;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::{Mutex as AsyncMutex, OwnedRwLockReadGuard, RwLock};
use url::Url;

const PUBLIC_KEY: &str = include_str!("../updater-public.key");
const MAX_DOWNLOAD: usize = 48 * 1024 * 1024;
const CHECK_SECONDS: i64 = 24 * 60 * 60;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Distribution {
    Installed,
    Portable,
    Development,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Checking,
    Available,
    Downloading,
    Ready,
    Installing,
    UpToDate,
    Error,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    revision: u64,
    current_version: String,
    distribution: Distribution,
    channel: &'static str,
    phase: Phase,
    auto_check: bool,
    checked_at: Option<String>,
    version: Option<String>,
    notes: Option<String>,
    published_at: Option<String>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    error: Option<String>,
    skipped_version: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Attempt {
    version: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    version: u32,
    auto_check: bool,
    skipped_version: Option<String>,
    last_check: Option<i64>,
    attempt: Option<Attempt>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            auto_check: true,
            skipped_version: None,
            last_check: None,
            attempt: None,
        }
    }
}

// Distinguish an absent field from explicit null ("restore version reminders").
fn present_optional<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreferencesPatch {
    auto_check: Option<bool>,
    #[serde(default, deserialize_with = "present_optional")]
    skip_version: Option<Option<String>>,
}

struct Inner {
    view: UpdateState,
    preferences: Preferences,
    writable: bool,
    candidate: Option<Update>,
    bytes: Option<Arc<Vec<u8>>>,
}
pub struct Updates {
    inner: Mutex<Inner>,
    operation: AsyncMutex<()>,
    path: PathBuf,
    activity: Arc<RwLock<()>>,
    installing: AtomicBool,
    #[cfg(test)]
    test_transport: Option<TestTransport>,
}
#[cfg(test)]
struct TestTransport {
    endpoint: Url,
    public_key: &'static str,
}
impl Updates {
    pub fn is_installing(&self) -> bool {
        self.installing.load(Ordering::Acquire)
    }
    pub fn new(path: PathBuf, current: String, distribution: Distribution) -> Self {
        let loaded = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Preferences>(&bytes)
                .map_err(|_| "更新配置损坏，原文件已保留。")
                .and_then(|p| {
                    if p.version == 1 {
                        Ok(p)
                    } else {
                        Err("更新配置版本不支持，原文件已保留。")
                    }
                }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
            Err(_) => Err("无法读取更新配置，原文件已保留。"),
        };
        let writable = loaded.is_ok();
        let mut error = loaded.as_ref().err().map(|e| e.to_string());
        let mut preferences = loaded.unwrap_or_else(|_| Preferences {
            auto_check: false,
            ..Default::default()
        });
        if let Some(attempt) = &preferences.attempt {
            if attempt.version == current {
                preferences.attempt = None;
            } else {
                error = Some(format!(
                    "上次尝试更新到 {}，当前仍为 {}；可重新检查、安装或从发布页手动修复。",
                    attempt.version, current
                ));
            }
        }
        let checked_at = preferences
            .last_check
            .and_then(|v| chrono::DateTime::from_timestamp(v, 0))
            .map(|v| v.to_rfc3339());
        let view = UpdateState {
            revision: 0,
            current_version: current,
            distribution,
            channel: "preview",
            phase: if error.is_some() {
                Phase::Error
            } else {
                Phase::Idle
            },
            auto_check: preferences.auto_check,
            checked_at,
            version: None,
            notes: None,
            published_at: None,
            downloaded_bytes: 0,
            total_bytes: None,
            error,
            skipped_version: preferences.skipped_version.clone(),
        };
        Self {
            inner: Mutex::new(Inner {
                view,
                preferences,
                writable,
                candidate: None,
                bytes: None,
            }),
            operation: AsyncMutex::new(()),
            path,
            activity: Arc::new(RwLock::new(())),
            installing: AtomicBool::new(false),
            #[cfg(test)]
            test_transport: None,
        }
    }
    fn change(&self, app: &AppHandle, change: impl FnOnce(&mut Inner)) -> UpdateState {
        let view = {
            let mut inner = self.inner.lock().unwrap();
            change(&mut inner);
            inner.view.revision += 1;
            inner.view.clone()
        };
        let _ = app.emit("subgauge:update", &view);
        sync_menu(app, &view);
        view
    }
    pub fn status(&self) -> UpdateState {
        self.inner.lock().unwrap().view.clone()
    }
    fn https_only(&self) -> bool {
        #[cfg(test)]
        if self.test_transport.is_some() {
            return false;
        }
        true
    }
    fn public_key(&self) -> &str {
        #[cfg(test)]
        if let Some(transport) = &self.test_transport {
            return transport.public_key;
        }
        PUBLIC_KEY.trim()
    }
    fn download_url(&self, url: &Url) -> Url {
        #[cfg(test)]
        if let Some(transport) = &self.test_transport {
            return transport.endpoint.join("package").unwrap();
        }
        url.clone()
    }
    fn save_preferences(&self, inner: &mut Inner, next: Preferences) -> Result<(), String> {
        if !inner.writable {
            return Err("更新配置无法保存，原文件已保留。".into());
        }
        let bytes = serde_json::to_vec_pretty(&next).map_err(|_| "更新配置编码失败")?;
        crate::core::atomic_write(&self.path, &bytes)?;
        inner.preferences = next;
        Ok(())
    }
    fn due(&self, now: i64) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.preferences.auto_check && check_due(inner.preferences.last_check, now)
    }
    pub async fn activity(&self) -> Result<OwnedRwLockReadGuard<()>, String> {
        if self.installing.load(Ordering::Acquire) {
            return Err("正在准备安装更新，请稍后操作。".into());
        }
        let guard = self.activity.clone().read_owned().await;
        if self.installing.load(Ordering::Acquire) {
            return Err("正在准备安装更新，请稍后操作。".into());
        }
        Ok(guard)
    }
    async fn check(&self, app: &AppHandle, manual: bool, preserve_download: bool) -> UpdateState {
        let Ok(_operation) = self.operation.try_lock() else {
            return self.status();
        };
        if preserve_download && self.inner.lock().unwrap().bytes.is_some() {
            return self.status();
        }
        if !manual && !self.due(Utc::now().timestamp()) {
            return self.status();
        }
        self.change(app, |i| {
            i.candidate = None;
            i.bytes = None;
            i.view.phase = Phase::Checking;
            i.view.error = None;
            i.view.version = None;
            i.view.notes = None;
            i.view.published_at = None;
            i.view.downloaded_bytes = 0;
            i.view.total_bytes = None;
        });
        let result = async {
            let https_only = self.https_only();
            let builder = app
                .updater_builder()
                .timeout(Duration::from_secs(20))
                .configure_client(move |client| {
                    client
                        .https_only(https_only)
                        .connect_timeout(Duration::from_secs(10))
                });
            #[cfg(test)]
            let builder = if let Some(transport) = &self.test_transport {
                builder
                    .endpoints(vec![transport.endpoint.clone()])
                    .map_err(|_| "测试更新地址不可用")?
            } else {
                builder
            };
            let updater = builder
                .build()
                .map_err(|_| "更新服务配置不可用".to_string())?;
            let update = updater.check().await.map_err(|_| {
                "检查更新失败，请检查网络后重试；更新渠道尚未发布时也会出现此状态。".to_string()
            })?;
            if let Some(update) = &update {
                validate_release(&update.version, &update.download_url)?;
            }
            Ok::<_, String>(update)
        }
        .await;
        self.change(app, |i| {
            let mut next = i.preferences.clone();
            next.last_check = Some(Utc::now().timestamp());
            // A failed preferences write must not turn a partial save into a success.
            let saved = self.save_preferences(i, next);
            i.view.checked_at = Some(Utc::now().to_rfc3339());
            match result {
                Ok(Some(update)) => {
                    i.view.phase = Phase::Available;
                    i.view.version = Some(update.version.clone());
                    i.view.notes = update
                        .body
                        .as_ref()
                        .map(|v| v.chars().take(8_000).collect());
                    i.view.published_at = update
                        .date
                        .and_then(|v| {
                            chrono::DateTime::from_timestamp(v.unix_timestamp(), v.nanosecond())
                        })
                        .map(|v| v.to_rfc3339());
                    i.candidate = Some(update);
                    i.view.error = saved
                        .err()
                        .map(|_| "更新检查成功，但检查时间未保存。".into());
                }
                Ok(None) => {
                    i.view.phase = Phase::UpToDate;
                    i.view.error = saved.err().map(|_| "检查完成，但检查时间未保存。".into());
                }
                Err(e) => {
                    i.view.phase = Phase::Error;
                    i.view.error = Some(e);
                }
            }
        })
    }
    async fn download(&self, app: &AppHandle, revision: u64) -> Result<UpdateState, String> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| "更新操作正在进行，请稍后重试。")?;
        let update = {
            let inner = self.inner.lock().unwrap();
            if inner.view.distribution != Distribution::Installed {
                return Err("此分发类型不支持内置安装，请从发布页下载对应版本。".into());
            }
            validate_action(&inner.view, revision, &[Phase::Available, Phase::Error])?;
            inner.candidate.clone().ok_or("请先重新检查更新。")?
        };
        self.change(app, |i| {
            i.bytes = None;
            i.view.phase = Phase::Downloading;
            i.view.error = None;
            i.view.downloaded_bytes = 0;
            i.view.total_bytes = None;
        });
        let result = self.download_bytes(app, &update).await;
        Ok(self.change(app, |i| match result {
            Ok(bytes) => {
                i.bytes = Some(Arc::new(bytes));
                i.view.phase = Phase::Ready;
            }
            Err(e) => {
                i.bytes = None;
                i.view.phase = Phase::Error;
                i.view.error = Some(e);
            }
        }))
    }
    async fn download_bytes(&self, app: &AppHandle, update: &Update) -> Result<Vec<u8>, String> {
        validate_release(&update.version, &update.download_url)?;
        let client = reqwest::Client::builder()
            .https_only(self.https_only())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 || !allowed_download_host(attempt.url()) {
                    attempt.error("更新下载跳转不可用")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| "无法创建更新下载连接")?;
        let mut response = client
            .get(self.download_url(&update.download_url))
            .send()
            .await
            .map_err(|_| "下载失败，请检查网络后重试。")?;
        if !response.status().is_success() {
            return Err("更新附件暂时不可下载，请稍后重试或打开发布页。".into());
        }
        let total = response.content_length();
        if total.is_some_and(|n| n > MAX_DOWNLOAD as u64) {
            return Err("更新文件超出允许大小，请从发布页手动安装。".into());
        }
        self.change(app, |i| i.view.total_bytes = total);
        let mut bytes = Vec::new();
        let mut last_progress = std::time::Instant::now();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "下载中断，请重新下载。")?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_DOWNLOAD {
                return Err("更新文件超出允许大小，请从发布页手动安装。".into());
            }
            bytes.extend_from_slice(&chunk);
            if last_progress.elapsed() >= Duration::from_millis(100) {
                self.change(app, |i| i.view.downloaded_bytes = bytes.len() as u64);
                last_progress = std::time::Instant::now();
            }
        }
        verify_package(
            &bytes,
            &update.signature,
            self.public_key(),
            &update.version,
        )?;
        self.change(app, |i| i.view.downloaded_bytes = bytes.len() as u64);
        Ok(bytes)
    }
    async fn install(&self, app: &AppHandle, revision: u64) -> Result<(), String> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| "更新操作正在进行，请稍后重试。")?;
        let (update, bytes) = {
            let inner = self.inner.lock().unwrap();
            if inner.view.distribution != Distribution::Installed {
                return Err("此分发类型不支持内置安装。".into());
            }
            validate_action(&inner.view, revision, &[Phase::Ready])?;
            (
                inner.candidate.clone().ok_or("请重新检查更新。")?,
                inner.bytes.clone().ok_or("请重新下载更新。")?,
            )
        };
        verify_package(
            &bytes,
            &update.signature,
            self.public_key(),
            &update.version,
        )?;
        self.installing.store(true, Ordering::Release);
        self.change(app, |i| {
            i.view.phase = Phase::Installing;
            i.view.error = None;
        });
        let result = async {
            let _guard =
                tokio::time::timeout(Duration::from_secs(30), self.activity.clone().write_owned())
                    .await
                    .map_err(|_| "仍有账号操作或同步未完成，请稍后重试。".to_string())?;
            crate::windows::on_ui(app.clone(), |app| {
                app.state::<crate::windows::Windows>().save_checked(&app)
            })
            .await?;
            app.state::<crate::core::Engine>().save_before_update()?;
            {
                let mut inner = self.inner.lock().unwrap();
                let mut next = inner.preferences.clone();
                next.attempt = Some(Attempt {
                    version: update.version.clone(),
                });
                self.save_preferences(&mut inner, next)?;
            }
            // install() does not verify bytes; this immutable buffer was verified above.
            // Official Windows updater exits after starting NSIS, so preparation must finish first.
            update.install(&*bytes).map_err(|_| {
                "无法启动更新安装器；旧版仍可使用，可重试或从发布页手动安装。".to_string()
            })
        }
        .await;
        if let Err(e) = result {
            self.installing.store(false, Ordering::Release);
            self.change(app, |i| {
                i.view.phase = Phase::Ready;
                i.view.error = Some(e.clone());
            });
            return Err(e);
        }
        Ok(())
    }
    fn preferences(&self, app: &AppHandle, patch: PreferencesPatch) -> Result<UpdateState, String> {
        if self.installing.load(Ordering::Acquire) {
            return Err("正在安装更新，请稍后重试。".into());
        }
        if let Some(Some(version)) = &patch.skip_version {
            parse_version(version)?;
        }
        let mut inner = self.inner.lock().unwrap();
        let mut next = inner.preferences.clone();
        if let Some(value) = patch.auto_check {
            next.auto_check = value;
        }
        if let Some(value) = patch.skip_version {
            next.skipped_version = value;
        }
        self.save_preferences(&mut inner, next)?;
        inner.view.auto_check = inner.preferences.auto_check;
        inner.view.skipped_version = inner.preferences.skipped_version.clone();
        inner.view.revision += 1;
        let view = inner.view.clone();
        drop(inner);
        let _ = app.emit("subgauge:update", &view);
        sync_menu(app, &view);
        Ok(view)
    }
}

fn parse_version(version: &str) -> Result<semver::Version, String> {
    if version.len() > 96 {
        return Err("更新版本格式不正确。".into());
    }
    semver::Version::parse(version.strip_prefix('v').unwrap_or(version))
        .map_err(|_| "更新版本格式不正确。".into())
}
fn sync_menu(app: &AppHandle, _state: &UpdateState) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<Updates>().status();
        if let Some(menu) = handle.try_state::<crate::UpdateMenu>() {
            let text = match &state.version {
                Some(version) if state.skipped_version.as_ref() != Some(version) => {
                    format!("有新版 {version} · 检查更新")
                }
                _ => "检查更新".into(),
            };
            let _ = menu.0.set_text(text);
            let _ = menu.0.set_enabled(state.phase != Phase::Installing);
        }
    });
}
fn check_due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|v| v > now || now.saturating_sub(v) >= CHECK_SECONDS)
}
fn validate_action(view: &UpdateState, revision: u64, phases: &[Phase]) -> Result<(), String> {
    if view.revision != revision {
        return Err("更新状态已改变，请确认当前版本后重试。".into());
    }
    if !phases.contains(&view.phase) {
        return Err("当前更新阶段不支持此操作。".into());
    }
    Ok(())
}
fn validate_release(version: &str, url: &Url) -> Result<(), String> {
    // SemVer allows '+' in build metadata; the release generator escapes it per segment.
    let version = parse_version(version)?.to_string().replace('+', "%2B");
    let expected =
        format!("/YuVerseX/SubGauge/releases/download/v{version}/SubGauge_{version}_x64-setup.exe");
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != expected
    {
        return Err("更新附件不是预期的 SubGauge 官方版本。".into());
    }
    Ok(())
}
fn allowed_download_host(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}
pub(crate) fn verify_package(
    bytes: &[u8],
    signature: &str,
    public_key: &str,
    version: &str,
) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_DOWNLOAD || signature.len() > 4096 {
        return Err("更新文件或签名大小不正确。".into());
    }
    let decode = |text: &str| -> Result<String, String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .map_err(|_| "更新签名格式不正确。")?;
        String::from_utf8(bytes).map_err(|_| "更新签名格式不正确。".into())
    };
    let key = minisign_verify::PublicKey::decode(&decode(public_key)?)
        .map_err(|_| "更新公钥格式不正确。")?;
    let signature = minisign_verify::Signature::decode(&decode(signature)?)
        .map_err(|_| "更新签名格式不正确。")?;
    key.verify(bytes, &signature, true)
        .map_err(|_| "更新签名验证失败，未执行安装。")?;
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .ok_or("更新签名缺少版本，未执行安装。")?;
    if parse_version(signed)? != parse_version(version)? {
        return Err("更新签名版本与公告不一致，未执行安装。".into());
    }
    Ok(())
}
pub fn distribution() -> Distribution {
    if cfg!(debug_assertions) {
        Distribution::Development
    } else if tauri::utils::platform::bundle_type() == Some(tauri::utils::config::BundleType::Nsis)
    {
        Distribution::Installed
    } else {
        Distribution::Portable
    }
}
pub fn installer_directory_argument(path: &std::path::Path) -> Result<String, String> {
    let directory = path
        .parent()
        .filter(|p| p.is_absolute())
        .ok_or("安装位置不可用")?;
    let text = directory.to_str().ok_or("安装位置格式不支持")?;
    if text.contains(['"', '\r', '\n', '\0']) {
        return Err("安装位置格式不支持".into());
    }
    Ok(format!("/D={text}"))
}
pub fn start_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        let mut ticker = tokio::time::interval(Duration::from_secs(60 * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let updates = app.state::<Updates>();
            if updates.due(Utc::now().timestamp()) {
                updates.check(&app, false, true).await;
            }
        }
    });
}
#[tauri::command]
pub fn update_status(updates: tauri::State<'_, Updates>) -> UpdateState {
    updates.status()
}
#[tauri::command]
pub async fn check_update(app: AppHandle, manual: Option<bool>) -> UpdateState {
    let manual = manual.unwrap_or(true);
    app.state::<Updates>().check(&app, manual, !manual).await
}
pub async fn check_from_tray(app: AppHandle) {
    app.state::<Updates>().check(&app, true, true).await;
}
#[tauri::command]
pub async fn download_update(app: AppHandle, revision: u64) -> Result<UpdateState, String> {
    app.state::<Updates>().download(&app, revision).await
}
#[tauri::command]
pub async fn install_update(app: AppHandle, revision: u64) -> Result<(), String> {
    app.state::<Updates>().install(&app, revision).await
}
#[tauri::command]
pub fn save_update_preferences(
    app: AppHandle,
    request: tauri::ipc::Request<'_>,
) -> Result<UpdateState, String> {
    let tauri::ipc::InvokeBody::Json(value) = request.body() else {
        return Err("更新偏好格式不正确。".into());
    };
    let patch = serde_json::from_value(value.clone()).map_err(|_| "更新偏好格式不正确。")?;
    app.state::<Updates>().preferences(&app, patch)
}
#[tauri::command]
pub fn open_update_release(app: AppHandle, revision: u64) -> Result<(), String> {
    let updates = app.state::<Updates>();
    let view = updates.status();
    if view.revision != revision {
        return Err("更新状态已改变，请重试。".into());
    }
    let version = view.version.ok_or("请先检查可用版本。")?;
    let version = parse_version(&version)?.to_string().replace('+', "%2B");
    let url = format!("https://github.com/YuVerseX/SubGauge/releases/tag/v{version}");
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let url: Vec<u16> = std::ffi::OsStr::new(&url)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let verb: Vec<u16> = std::ffi::OsStr::new("open")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result as isize <= 32 {
            return Err("无法打开发布页，请从 GitHub 项目页下载。".into());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        return Err("此版本仅支持 Windows。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
