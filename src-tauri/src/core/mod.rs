mod client;
mod stats;
mod storage;
pub(crate) use storage::atomic_write;
#[cfg(test)]
mod tests;
mod types;
use chrono::{DateTime, Duration, Utc};
use client::{Api, ApiError, Session};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::Mutex as AsyncMutex;
pub use types::*;

#[derive(Clone, Serialize, Deserialize)]
struct SavedAccount {
    summary: AccountSummary,
    encrypted_session: Option<Vec<u8>>,
    snapshot: Option<UsageSnapshot>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Config {
    version: u32,
    accounts: Vec<SavedAccount>,
    current: Option<String>,
    settings: AppSettings,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: vec![],
            current: None,
            settings: AppSettings::default(),
        }
    }
}
struct Runtime {
    session: AsyncMutex<Option<Session>>,
    refresh: AsyncMutex<()>,
    range: Mutex<UsageRange>,
    last_recent: Mutex<i64>,
    last_summary: Mutex<i64>,
    retry_at: Mutex<i64>,
    failures: Mutex<u32>,
    analysis: AsyncMutex<Option<(String, i64, AnalysisResult)>>,
    records: AsyncMutex<Option<RecordCursor>>,
}
#[derive(Clone)]
struct RecordCursor {
    key: String,
    created_at: i64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    date_total: u64,
    first: u64,
    last: u64,
    head: Vec<(i64, String, String)>,
}
struct WindowRecordPage {
    items: Vec<UsageRecord>,
    total: u64,
    complete: bool,
    message: Option<String>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}
impl Runtime {
    fn new(session: Option<Session>, range: UsageRange) -> Self {
        Self {
            session: AsyncMutex::new(session),
            refresh: AsyncMutex::new(()),
            range: Mutex::new(range),
            last_recent: Mutex::new(0),
            last_summary: Mutex::new(0),
            retry_at: Mutex::new(0),
            failures: Mutex::new(0),
            analysis: AsyncMutex::new(None),
            records: AsyncMutex::new(None),
        }
    }
}
struct Pending {
    site: String,
    alias: String,
    remember: bool,
    temp_token: String,
    expires: i64,
}
struct State {
    config: Config,
    runtime: HashMap<String, Arc<Runtime>>,
    pending: HashMap<String, Pending>,
    generation: u64,
}
pub struct Engine {
    path: PathBuf,
    api: Api,
    state: Mutex<State>,
    disk: Mutex<()>,
}
impl Engine {
    pub fn new(data_dir: PathBuf) -> Result<Self, String> {
        let path = data_dir.join("accounts.v1.json");
        let mut config = if path.exists() {
            serde_json::from_slice::<Config>(&std::fs::read(&path).map_err(|_| "无法读取账号配置")?)
                .map_err(|_| "账号配置损坏，已保留原文件，请备份后检查。")?
        } else {
            Config::default()
        };
        if config.version != 1 {
            return Err("配置来自不兼容的应用版本，已保留原文件。".into());
        }
        let mut runtime = HashMap::new();
        for saved in &mut config.accounts {
            let session = saved
                .encrypted_session
                .as_ref()
                .and_then(|bytes| storage::unprotect(bytes).ok())
                .and_then(|bytes| serde_json::from_slice::<Session>(&bytes).ok());
            saved.summary.needs_login = session.is_none();
            if let Some(snapshot) = &mut saved.snapshot {
                snapshot.sync.state = if session.is_none() {
                    "needsLogin"
                } else {
                    "stale"
                }
                .into();
                snapshot.sync.message = Some("显示上次结果，等待重新同步。".into());
            }
            runtime.insert(
                saved.summary.id.clone(),
                Arc::new(Runtime::new(
                    session,
                    saved.summary.preferences.default_range,
                )),
            );
        }
        Ok(Self {
            path,
            api: Api::new()?,
            state: Mutex::new(State {
                config,
                runtime,
                pending: HashMap::new(),
                generation: 0,
            }),
            disk: Mutex::new(()),
        })
    }
    fn persist(&self) -> Result<(), String> {
        let _disk = self.disk.lock().unwrap();
        let mut config = self.state.lock().unwrap().config.clone();
        config.accounts.retain(|a| !a.summary.demo);
        if !config
            .accounts
            .iter()
            .any(|a| Some(&a.summary.id) == config.current.as_ref())
        {
            config.current = config.accounts.first().map(|a| a.summary.id.clone());
        }
        let bytes = serde_json::to_vec_pretty(&config).map_err(|_| "配置编码失败")?;
        storage::atomic_write(&self.path, &bytes)
    }
    fn context(&self, id: &str) -> Result<(AccountSummary, Arc<Runtime>), String> {
        let state = self.state.lock().unwrap();
        let a = state
            .config
            .accounts
            .iter()
            .find(|a| a.summary.id == id)
            .ok_or("账号不存在")?;
        let runtime = state.runtime.get(id).cloned().ok_or("账号不可用")?;
        Ok((a.summary.clone(), runtime))
    }
    pub async fn bootstrap(&self) -> Bootstrap {
        let s = self.state.lock().unwrap();
        let selected_range = s
            .config
            .current
            .as_ref()
            .and_then(|id| s.runtime.get(id))
            .map(|rt| *rt.range.lock().unwrap())
            .unwrap_or_default();
        let mut snapshot = s
            .config
            .accounts
            .iter()
            .find(|a| Some(&a.summary.id) == s.config.current.as_ref())
            .and_then(|a| a.snapshot.clone())
            .filter(|v| v.range == selected_range);
        if let Some(v) = &mut snapshot {
            v.generation = s.generation;
        }
        Bootstrap {
            accounts: s
                .config
                .accounts
                .iter()
                .map(|a| a.summary.clone())
                .collect(),
            current_account_id: s.config.current.clone(),
            selected_range,
            settings: s.config.settings.clone(),
            snapshot,
            generation: s.generation,
        }
    }
    pub async fn login(&self, input: LoginInput) -> Result<LoginOutcome, String> {
        let site = client::normalize_site(&input.site)?;
        if input.email.trim().is_empty() || input.password.is_empty() {
            return Err("请输入邮箱和密码。".into());
        }
        let v = self
            .api
            .raw(
                &site,
                "/auth/login",
                None,
                Some(json!({"email":input.email.trim(),"password":input.password})),
                &[],
            )
            .await
            .map_err(|e| e.message)?;
        if v["requires_2fa"].as_bool() == Some(true) {
            let token = v["temp_token"].as_str().ok_or("站点二次验证流程不兼容")?;
            let challenge = uuid::Uuid::new_v4().to_string();
            let mut s = self.state.lock().unwrap();
            s.pending.retain(|_, p| p.expires > Utc::now().timestamp());
            if s.pending.len() >= 8 {
                return Err("待验证登录过多，请稍后重试。".into());
            }
            s.pending.insert(
                challenge.clone(),
                Pending {
                    site,
                    alias: input.alias,
                    remember: input.remember,
                    temp_token: token.into(),
                    expires: Utc::now().timestamp() + 300,
                },
            );
            return Ok(LoginOutcome {
                status: "twoFactor".into(),
                challenge_id: Some(challenge),
                masked_email: v["user_email_masked"].as_str().map(str::to_owned),
                state: None,
            });
        }
        self.finish_login(site, input.alias, input.remember, v)
            .await
    }
    pub async fn complete_2fa(
        &self,
        challenge_id: String,
        code: String,
    ) -> Result<LoginOutcome, String> {
        let (site, alias, remember, token) = {
            let s = self.state.lock().unwrap();
            let p = s
                .pending
                .get(&challenge_id)
                .filter(|p| p.expires > Utc::now().timestamp())
                .ok_or("二次验证已过期，请重新登录。")?;
            (
                p.site.clone(),
                p.alias.clone(),
                p.remember,
                p.temp_token.clone(),
            )
        };
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Err("请输入六位二次验证码。".into());
        }
        let v = self
            .api
            .raw(
                &site,
                "/auth/login/2fa",
                None,
                Some(json!({"temp_token":token,"totp_code":code})),
                &[],
            )
            .await
            .map_err(|e| e.message)?;
        self.state.lock().unwrap().pending.remove(&challenge_id);
        self.finish_login(site, alias, remember, v).await
    }
    async fn finish_login(
        &self,
        site: String,
        alias: String,
        remember: bool,
        v: Value,
    ) -> Result<LoginOutcome, String> {
        let mut session = Api::session(&v)?;
        let profile = self
            .api
            .request(&site, "/user/profile", &mut session, &[])
            .await
            .map_err(|e| e.message)?;
        let user_id = profile["id"]
            .as_i64()
            .ok_or("站点未返回用户 ID，未保存账号。")?;
        let email = profile["email"]
            .as_str()
            .ok_or("站点未返回邮箱")?
            .to_owned();
        let id = format!("{site}#{user_id}");
        let encrypted = if remember {
            Some(storage::protect(
                &serde_json::to_vec(&session).map_err(|_| "会话编码失败")?,
            )?)
        } else {
            None
        };
        {
            let mut s = self.state.lock().unwrap();
            let existing = s.config.accounts.iter().find(|a| a.summary.id == id);
            let mut preferences = existing
                .map(|a| a.summary.preferences.clone())
                .unwrap_or_default();
            if !alias.trim().is_empty() {
                preferences.alias = alias.trim().chars().take(40).collect();
            }
            if preferences.alias.is_empty() {
                preferences.alias = url::Url::parse(&site)
                    .ok()
                    .and_then(|v| v.host_str().map(str::to_owned))
                    .unwrap_or_else(|| "我的账号".into());
            }
            let summary = AccountSummary {
                id: id.clone(),
                site,
                email,
                role: profile["role"].as_str().unwrap_or("user").into(),
                user_id,
                preferences: preferences.clone(),
                needs_login: false,
                demo: false,
            };
            let saved = SavedAccount {
                summary,
                encrypted_session: encrypted,
                snapshot: existing.and_then(|a| a.snapshot.clone()),
            };
            s.config.accounts.retain(|a| a.summary.id != id);
            s.config.accounts.push(saved);
            s.runtime.insert(
                id.clone(),
                Arc::new(Runtime::new(Some(session), preferences.default_range)),
            );
            s.config.current = Some(id);
            s.generation += 1;
        }
        self.persist()?;
        Ok(LoginOutcome {
            status: "success".into(),
            challenge_id: None,
            masked_email: None,
            state: Some(self.bootstrap().await),
        })
    }
    pub async fn switch_account(&self, id: String) -> Result<Bootstrap, String> {
        let (account, rt) = self.context(&id)?;
        *rt.range.lock().unwrap() = account.preferences.default_range;
        {
            let mut s = self.state.lock().unwrap();
            s.config.current = Some(id);
            s.generation += 1;
        }
        self.persist()?;
        Ok(self.bootstrap().await)
    }
    pub async fn save_preferences(
        &self,
        id: String,
        mut preferences: AccountPreferences,
    ) -> Result<Bootstrap, String> {
        stats::timezone(&preferences.timezone)?;
        if !(1..=1440).contains(&preferences.recent_minutes) {
            return Err("最近窗口应为 1～1440 分钟。".into());
        }
        let allowed = ["balance", "cost", "requests", "tokens", "cache"];
        let mut seen = HashSet::new();
        preferences
            .metrics
            .retain(|v| allowed.contains(&v.as_str()) && seen.insert(v.clone()));
        if preferences.metrics.is_empty() {
            return Err("至少保留一项指标。".into());
        }
        preferences.alias = preferences.alias.trim().chars().take(40).collect();
        if preferences.alias.is_empty() {
            return Err("请输入账号名称。".into());
        }
        let (_, rt) = self.context(&id)?;
        // Relogin can replace this runtime while an analysis request holds its lock.
        let mut analysis = rt.analysis.lock().await;
        {
            let mut s = self.state.lock().unwrap();
            if !s
                .runtime
                .get(&id)
                .is_some_and(|live| Arc::ptr_eq(live, &rt))
            {
                return Err("账号会话已变更，请重试保存显示设置。".into());
            }
            let a = s
                .config
                .accounts
                .iter_mut()
                .find(|a| a.summary.id == id)
                .ok_or("账号不存在")?;
            *rt.range.lock().unwrap() = preferences.default_range;
            *rt.last_summary.lock().unwrap() = 0;
            *rt.last_recent.lock().unwrap() = 0;
            *analysis = None;
            a.summary.preferences = preferences;
            a.snapshot = None;
            s.generation += 1;
        }
        drop(analysis);
        self.persist()?;
        Ok(self.bootstrap().await)
    }
    pub async fn save_settings(&self, settings: AppSettings) -> Result<Bootstrap, String> {
        if !settings.opacity.is_finite()
            || !(0.2..=1.0).contains(&settings.opacity)
            || !(5..=3600).contains(&settings.recent_refresh_seconds)
            || !(10..=3600).contains(&settings.summary_refresh_seconds)
            || !(30..=86400).contains(&settings.background_refresh_seconds)
        {
            return Err("设置超出允许范围。".into());
        }
        self.state.lock().unwrap().config.settings = settings;
        self.persist()?;
        Ok(self.bootstrap().await)
    }
    pub async fn remove_account(&self, id: String) -> Result<Bootstrap, String> {
        let (_, expected) = self.context(&id)?;
        self.logout(id.clone()).await?;
        {
            let mut s = self.state.lock().unwrap();
            if s.runtime
                .get(&id)
                .is_some_and(|live| Arc::ptr_eq(live, &expected))
            {
                s.config.accounts.retain(|a| a.summary.id != id);
                s.runtime.remove(&id);
                if s.config.current.as_ref() == Some(&id) {
                    s.config.current = s.config.accounts.first().map(|a| a.summary.id.clone());
                }
                s.generation += 1;
            }
        }
        self.persist()?;
        Ok(self.bootstrap().await)
    }
    pub async fn logout(&self, id: String) -> Result<Bootstrap, String> {
        let (a, rt) = self.context(&id)?;
        let _refresh = rt.refresh.lock().await;
        let session = rt.session.lock().await.take();
        {
            let mut s = self.state.lock().unwrap();
            if s.runtime
                .get(&id)
                .is_some_and(|live| Arc::ptr_eq(live, &rt))
            {
                if let Some(a) = s.config.accounts.iter_mut().find(|a| a.summary.id == id) {
                    a.encrypted_session = None;
                    a.summary.needs_login = true;
                    if let Some(v) = &mut a.snapshot {
                        v.sync.state = "needsLogin".into();
                        v.sync.message = Some("账号已退出，请重新登录。".into());
                    }
                }
            }
        }
        self.persist()?;
        if let Some(token) = session.and_then(|s| s.refresh_token) {
            let _ = self
                .api
                .raw(
                    &a.site,
                    "/auth/logout",
                    None,
                    Some(json!({"refresh_token":token})),
                    &[],
                )
                .await;
        }
        Ok(self.bootstrap().await)
    }
    async fn get(
        &self,
        a: &AccountSummary,
        rt: &Runtime,
        path: &str,
        query: &[(String, String)],
    ) -> Result<Value, ApiError> {
        let mut guard = rt.session.lock().await;
        let session = guard.as_mut().ok_or(ApiError {
            message: "请重新登录该账号。".into(),
            unauthorized: true,
            retry_after: None,
        })?;
        // Check under the session lock: requests already waiting for this account
        // must observe a preceding request's rate limit before sending anything.
        let remaining = *rt.retry_at.lock().unwrap() - Utc::now().timestamp();
        if remaining > 0 {
            return Err(ApiError {
                message: format!("账号请求已暂缓，{remaining} 秒后自动重试。"),
                unauthorized: false,
                retry_after: Some(remaining as u64),
            });
        }
        let old = session.access_token.clone();
        let result = self.api.request(&a.site, path, session, query).await;
        if let Some(error) = result.as_ref().err().filter(|e| e.retry_after.is_some()) {
            let deadline = Utc::now().timestamp() + error.retry_after.unwrap() as i64;
            {
                let mut retry_at = rt.retry_at.lock().unwrap();
                *retry_at = (*retry_at).max(deadline);
            }
            let changed = {
                let mut s = self.state.lock().unwrap();
                if s.runtime
                    .get(&a.id)
                    .is_some_and(|live| std::ptr::eq(Arc::as_ptr(live), rt))
                {
                    s.config
                        .accounts
                        .iter_mut()
                        .find(|v| v.summary.id == a.id)
                        .and_then(|saved| saved.snapshot.as_mut())
                        .map(|snapshot| {
                            snapshot.sync.state = "stale".into();
                            snapshot.sync.message = Some(error.message.clone());
                        })
                        .is_some()
                } else {
                    false
                }
            };
            if changed {
                self.persist().map_err(ApiError::from)?;
            }
        }
        if session.access_token != old {
            let remember = {
                let s = self.state.lock().unwrap();
                s.runtime
                    .get(&a.id)
                    .is_some_and(|live| std::ptr::eq(Arc::as_ptr(live), rt))
                    && s.config
                        .accounts
                        .iter()
                        .find(|v| v.summary.id == a.id)
                        .is_some_and(|v| v.encrypted_session.is_some())
            };
            if remember {
                let bytes =
                    serde_json::to_vec(session).map_err(|_| ApiError::from("会话编码失败"))?;
                let encrypted = storage::protect(&bytes).map_err(ApiError::from)?;
                {
                    let mut s = self.state.lock().unwrap();
                    if s.runtime
                        .get(&a.id)
                        .is_some_and(|live| std::ptr::eq(Arc::as_ptr(live), rt))
                    {
                        if let Some(saved) =
                            s.config.accounts.iter_mut().find(|v| v.summary.id == a.id)
                        {
                            saved.encrypted_session = Some(encrypted);
                        }
                    }
                }
                self.persist().map_err(ApiError::from)?;
            }
        }
        if result.as_ref().is_err_and(|e| e.unauthorized) {
            *guard = None;
            {
                let mut s = self.state.lock().unwrap();
                if s.runtime
                    .get(&a.id)
                    .is_some_and(|live| std::ptr::eq(Arc::as_ptr(live), rt))
                {
                    if let Some(saved) = s.config.accounts.iter_mut().find(|v| v.summary.id == a.id)
                    {
                        saved.summary.needs_login = true;
                        saved.encrypted_session = None;
                        if let Some(snapshot) = &mut saved.snapshot {
                            snapshot.sync.state = "needsLogin".into();
                            snapshot.sync.message = Some("登录已过期，请重新登录该账号。".into());
                        }
                    }
                }
            }
            self.persist().map_err(ApiError::from)?;
        }
        result
    }
    fn dates(
        a: &AccountSummary,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<(String, String)>, ApiError> {
        let tz = stats::timezone(&a.preferences.timezone).map_err(ApiError::from)?;
        Ok(vec![
            (
                "start_date".into(),
                start.with_timezone(&tz).format("%Y-%m-%d").to_string(),
            ),
            (
                "end_date".into(),
                end.with_timezone(&tz).format("%Y-%m-%d").to_string(),
            ),
            ("timezone".into(), a.preferences.timezone.clone()),
        ])
    }
    fn filters(query: &mut Vec<(String, String)>, key: Option<i64>, model: Option<&str>) {
        if let Some(key) = key {
            query.push(("api_key_id".into(), key.to_string()));
        }
        if let Some(model) = model.filter(|s| !s.trim().is_empty()) {
            query.push(("model".into(), model.into()));
        }
    }
    async fn page(
        &self,
        a: &AccountSummary,
        rt: &Runtime,
        mut query: Vec<(String, String)>,
        page: u32,
        size: u32,
    ) -> Result<(Vec<UsageRecord>, u64), ApiError> {
        query.extend([
            ("page".into(), page.to_string()),
            ("page_size".into(), size.to_string()),
            ("sort_by".into(), "created_at".into()),
            ("sort_order".into(), "desc".into()),
        ]);
        let v = self.get(a, rt, "/usage", &query).await?;
        let items = v["items"]
            .as_array()
            .ok_or(ApiError::from("站点记录分页格式不兼容。"))?;
        let total = v["total"]
            .as_u64()
            .ok_or(ApiError::from("站点未返回记录总数。"))?;
        if v["page"].as_u64() != Some(u64::from(page))
            || v["page_size"].as_u64() != Some(u64::from(size))
        {
            return Err("站点返回的分页参数与请求不一致，无法确认记录位置。".into());
        }
        Ok((
            items
                .iter()
                .map(stats::record)
                .collect::<Result<_, _>>()
                .map_err(ApiError::from)?,
            total,
        ))
    }
    fn record_fingerprint(rows: &[UsageRecord]) -> Vec<(i64, String, String)> {
        rows.iter()
            .map(|r| (r.id, r.created_at.clone(), r.totals.cost.clone()))
            .collect()
    }
    fn valid_record_page(
        rows: &[UsageRecord],
        total: u64,
        page: u32,
        size: u32,
        key: Option<i64>,
        model: Option<&str>,
    ) -> bool {
        let offset = u64::from(page - 1) * u64::from(size);
        let expected = total.saturating_sub(offset).min(u64::from(size));
        if rows.len() as u64 != expected {
            return false;
        }
        let mut ids = HashSet::new();
        let mut previous = None;
        for row in rows {
            let Ok(time) = DateTime::parse_from_rfc3339(&row.created_at) else {
                return false;
            };
            if !ids.insert(row.id)
                || previous.is_some_and(|p| time > p)
                || key.is_some_and(|id| row.api_key_id != id)
                || model
                    .filter(|m| !m.trim().is_empty())
                    .is_some_and(|m| row.model != m)
            {
                return false;
            }
            previous = Some(time);
        }
        true
    }
    /// Find the number of date-filtered, descending records at or after a timestamp.
    /// One-row binary probes avoid downloading every record in a rolling interval.
    async fn record_rank(
        &self,
        a: &AccountSummary,
        rt: &Runtime,
        params: &[(String, String)],
        first_page: (&[UsageRecord], u64),
        threshold: DateTime<Utc>,
        query: &RecordQuery,
    ) -> Result<Option<u64>, ApiError> {
        let (head, total) = first_page;
        let mut lo = 0;
        let mut hi = total;
        for row in head {
            let time = DateTime::parse_from_rfc3339(&row.created_at)
                .map_err(|_| ApiError::from("记录时间格式无效"))?;
            if time < threshold {
                hi = lo;
                break;
            }
            lo += 1;
        }
        while lo < hi {
            let middle = lo + (hi - lo) / 2;
            let page = u32::try_from(middle + 1)
                .map_err(|_| ApiError::from("站点记录数超出分页接口支持范围。"))?;
            let (rows, observed_total) = self.page(a, rt, params.to_vec(), page, 1).await?;
            if observed_total != total
                || !Self::valid_record_page(
                    &rows,
                    total,
                    page,
                    1,
                    query.api_key_id,
                    query.model.as_deref(),
                )
            {
                return Ok(None);
            }
            let time = DateTime::parse_from_rfc3339(&rows[0].created_at)
                .map_err(|_| ApiError::from("记录时间格式无效"))?;
            if time >= threshold {
                lo = middle + 1;
            } else {
                hi = middle;
            }
        }
        Ok(Some(lo))
    }
    async fn window_records(
        &self,
        a: &AccountSummary,
        rt: &Runtime,
        query: &RecordQuery,
        page: u32,
        size: u32,
        now: DateTime<Utc>,
    ) -> Result<WindowRecordPage, ApiError> {
        let cache_key = format!(
            "{:?}|{:?}|{:?}|{}|{}",
            query.range,
            query.api_key_id,
            query.model,
            a.preferences.timezone,
            a.preferences.recent_minutes,
        );
        let mut cache = rt.records.lock().await;
        let previous = cache.as_ref().filter(|c| c.key == cache_key).cloned();
        let expired = previous
            .as_ref()
            .is_some_and(|c| now.timestamp() - c.created_at >= 60);
        if page > 1 && expired {
            *cache = None;
            let old = previous.unwrap();
            return Ok(WindowRecordPage {
                items: vec![],
                total: 0,
                complete: false,
                message: Some("分页范围已过期，请刷新第一页。".into()),
                start: old.start,
                end: old.end,
            });
        }
        let reuse = (page > 1).then_some(previous).flatten();
        let (start, end) = reuse.as_ref().map(|c| (c.start, c.end)).unwrap_or(
            stats::bounds(
                query.range,
                a.preferences.recent_minutes,
                stats::timezone(&a.preferences.timezone).map_err(ApiError::from)?,
                now,
            )
            .map_err(ApiError::from)?,
        );
        let mut params = Self::dates(a, start, end - Duration::nanoseconds(1))?;
        Self::filters(&mut params, query.api_key_id, query.model.as_deref());
        for _ in 0..2 {
            let (head, date_total) = self.page(a, rt, params.clone(), 1, 100).await?;
            if !Self::valid_record_page(
                &head,
                date_total,
                1,
                100,
                query.api_key_id,
                query.model.as_deref(),
            ) {
                continue;
            }
            let cursor = if let Some(cursor) = &reuse {
                if date_total != cursor.date_total || Self::record_fingerprint(&head) != cursor.head
                {
                    break;
                }
                cursor.clone()
            } else {
                let Some(first) = self
                    .record_rank(a, rt, &params, (&head, date_total), end, query)
                    .await?
                else {
                    continue;
                };
                let Some(last) = self
                    .record_rank(a, rt, &params, (&head, date_total), start, query)
                    .await?
                else {
                    continue;
                };
                if first > last {
                    continue;
                }
                RecordCursor {
                    key: cache_key.clone(),
                    created_at: now.timestamp(),
                    start,
                    end,
                    date_total,
                    first,
                    last,
                    head: Self::record_fingerprint(&head),
                }
            };
            let total = cursor.last - cursor.first;
            let offset = cursor
                .first
                .saturating_add(u64::from(page - 1) * u64::from(size));
            let length = cursor.last.saturating_sub(offset).min(u64::from(size));
            let mut fetched = vec![];
            let mut stable = true;
            if length > 0 {
                let first_page = offset / 100 + 1;
                let last_page = (offset + length - 1) / 100 + 1;
                for number in first_page..=last_page {
                    let number = u32::try_from(number)
                        .map_err(|_| ApiError::from("目标页超出分页接口支持范围。"))?;
                    let (rows, observed_total) = if number == 1 {
                        (head.clone(), date_total)
                    } else {
                        self.page(a, rt, params.clone(), number, 100).await?
                    };
                    if observed_total != date_total
                        || !Self::valid_record_page(
                            &rows,
                            date_total,
                            number,
                            100,
                            query.api_key_id,
                            query.model.as_deref(),
                        )
                    {
                        stable = false;
                        break;
                    }
                    fetched.push((number, rows));
                }
            }
            let items = fetched
                .iter()
                .flat_map(|(_, rows)| rows.iter().cloned())
                .skip((offset % 100) as usize)
                .take(length as usize)
                .collect::<Vec<_>>();
            let mut ids = HashSet::new();
            stable &= items.len() as u64 == length
                && items
                    .iter()
                    .all(|r| ids.insert(r.id) && stats::within(r, start, end));
            // Verify both the global head and requested pages. Equal timestamps can otherwise
            // hide a reordered target page even when the head and total remain unchanged.
            for (number, before) in &fetched {
                if *number == 1 {
                    continue;
                }
                let (after, observed_total) =
                    self.page(a, rt, params.clone(), *number, 100).await?;
                stable &= observed_total == date_total
                    && Self::record_fingerprint(before) == Self::record_fingerprint(&after);
            }
            let (verify, observed_total) = self.page(a, rt, params.clone(), 1, 100).await?;
            stable &=
                observed_total == date_total && Self::record_fingerprint(&verify) == cursor.head;
            if stable {
                let tz = stats::timezone(&a.preferences.timezone).map_err(ApiError::from)?;
                let message = Some(format!(
                    "分页范围固定为 {} 至 {}（{}）；刷新第一页更新范围。",
                    start.with_timezone(&tz).format("%m-%d %H:%M:%S"),
                    end.with_timezone(&tz).format("%m-%d %H:%M:%S"),
                    a.preferences.timezone
                ));
                *cache = Some(cursor);
                return Ok(WindowRecordPage {
                    items,
                    total,
                    complete: true,
                    message,
                    start,
                    end,
                });
            }
            if reuse.is_some() {
                break;
            }
        }
        *cache = None;
        Ok(WindowRecordPage {
            items: vec![],
            total: 0,
            complete: false,
            message: Some("记录分页在读取期间发生变化，请刷新第一页重试。".into()),
            start,
            end,
        })
    }
    /// Read all pages covering the interval, deduplicate overlap and verify the head again.
    /// Unstable pagination retries once; bounded/truncated reads are explicitly incomplete.
    async fn collect(
        &self,
        a: &AccountSummary,
        rt: &Runtime,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        key: Option<i64>,
        model: Option<&str>,
    ) -> Result<(Vec<UsageRecord>, bool), ApiError> {
        let mut query = Self::dates(a, start, end - Duration::nanoseconds(1))?;
        Self::filters(&mut query, key, model);
        let mut final_rows = vec![];
        for _ in 0..2 {
            let mut rows = HashMap::new();
            let mut head = vec![];
            let mut initial_total = 0;
            let mut reached = false;
            let mut entire_date_range = false;
            let mut duplicates = false;
            for page in 1..=50 {
                let (items, total) = self.page(a, rt, query.clone(), page, 100).await?;
                if page == 1 {
                    initial_total = total;
                    head = items
                        .iter()
                        .map(|r| (r.id, r.totals.cost.clone()))
                        .collect::<Vec<_>>();
                }
                if total != initial_total {
                    break;
                }
                let older = items
                    .last()
                    .and_then(|r| DateTime::parse_from_rfc3339(&r.created_at).ok())
                    .is_some_and(|d| d < start);
                let length = items.len();
                for r in items {
                    duplicates |= rows.insert(r.id, r).is_some();
                }
                if length < 100 || u64::from(page) * 100 >= total || older {
                    reached = true;
                    entire_date_range = length < 100 || u64::from(page) * 100 >= total;
                    break;
                }
            }
            let (verify, total) = self.page(a, rt, query.clone(), 1, 100).await?;
            let stable = total == initial_total
                && head
                    == verify
                        .iter()
                        .map(|r| (r.id, r.totals.cost.clone()))
                        .collect::<Vec<_>>();
            for r in verify {
                rows.insert(r.id, r);
            }
            let count_matches = if entire_date_range {
                rows.len() as u64 == total
            } else {
                !duplicates
            };
            final_rows = rows
                .into_values()
                .filter(|r| stats::within(r, start, end))
                .collect();
            final_rows.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
            if reached && stable && count_matches {
                return Ok((final_rows, true));
            }
        }
        Ok((final_rows, false))
    }
    async fn summary(
        &self,
        a: &AccountSummary,
        rt: &Runtime,
        range: UsageRange,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<(Totals, bool), ApiError> {
        if range == UsageRange::Recent {
            let (rows, complete) = self.collect(a, rt, start, end, None, None).await?;
            return Ok((stats::aggregate(&rows, start, end), complete));
        }
        if range != UsageRange::Week {
            let query = Self::dates(a, start, end)?;
            let v = self.get(a, rt, "/usage/stats", &query).await?;
            return Ok((Totals::from_summary(&v).map_err(ApiError::from)?, true));
        }
        let tz = stats::timezone(&a.preferences.timezone).map_err(ApiError::from)?;
        let start_day = start.with_timezone(&tz).date_naive();
        let next = stats::midnight(start_day.succ_opt().ok_or(ApiError::from("日期无效"))?, tz)
            .map_err(ApiError::from)?;
        let end_day =
            stats::midnight(end.with_timezone(&tz).date_naive(), tz).map_err(ApiError::from)?;
        let (first, first_ok) = self.collect(a, rt, start, next, None, None).await?;
        let (last, last_ok) = self.collect(a, rt, end_day, end, None, None).await?;
        let mut totals = stats::aggregate(&first, start, next);
        totals.add(&stats::aggregate(&last, end_day, end));
        if next < end_day {
            let query = Self::dates(a, next, end_day - Duration::seconds(1))?;
            let v = self.get(a, rt, "/usage/stats", &query).await?;
            totals.add(&Totals::from_summary(&v).map_err(ApiError::from)?);
        }
        Ok((totals, first_ok && last_ok))
    }
    pub async fn refresh(
        &self,
        id: Option<String>,
        range: Option<UsageRange>,
        force: bool,
    ) -> Result<Bootstrap, String> {
        let id = id.or_else(|| self.state.lock().unwrap().config.current.clone());
        if let Some(id) = id {
            let (_, rt) = self.context(&id)?;
            if let Some(range) = range {
                let changed = *rt.range.lock().unwrap() != range;
                *rt.range.lock().unwrap() = range;
                if changed {
                    *rt.last_summary.lock().unwrap() = 0;
                    self.state.lock().unwrap().generation += 1;
                }
            }
            self.refresh_account(&id, force).await?;
        }
        Ok(self.bootstrap().await)
    }
    async fn refresh_account(&self, id: &str, force: bool) -> Result<(), String> {
        let (a, rt) = self.context(id)?;
        let Ok(_refresh) = rt.refresh.try_lock() else {
            return Ok(());
        };
        let now = Utc::now();
        let now_ts = now.timestamp();
        if now_ts < *rt.retry_at.lock().unwrap() {
            return Ok(());
        }
        let (settings, is_current, generation, old) = {
            let s = self.state.lock().unwrap();
            (
                s.config.settings.clone(),
                s.config.current.as_deref() == Some(id),
                s.generation,
                s.config
                    .accounts
                    .iter()
                    .find(|a| a.summary.id == id)
                    .and_then(|a| a.snapshot.clone()),
            )
        };
        let range = *rt.range.lock().unwrap();
        let retry_partial = old
            .as_ref()
            .is_some_and(|v| matches!(v.sync.state.as_str(), "stale" | "partial" | "needsLogin"));
        let recent_due = force
            || retry_partial
            || now_ts - *rt.last_recent.lock().unwrap()
                >= if is_current {
                    settings.recent_refresh_seconds
                } else {
                    settings.background_refresh_seconds
                } as i64;
        let summary_due = force
            || retry_partial
            || now_ts - *rt.last_summary.lock().unwrap()
                >= if is_current {
                    settings.summary_refresh_seconds
                } else {
                    settings.background_refresh_seconds
                } as i64
            || old.as_ref().is_none_or(|v| v.range != range);
        let recent_due = recent_due || summary_due;
        if !recent_due && !summary_due {
            return Ok(());
        }
        if a.demo {
            let snapshot = self.demo_snapshot(&a, range, generation)?;
            let mut s = self.state.lock().unwrap();
            if let Some(saved) = s.config.accounts.iter_mut().find(|v| v.summary.id == id) {
                saved.snapshot = Some(snapshot);
            }
            *rt.last_recent.lock().unwrap() = now_ts;
            *rt.last_summary.lock().unwrap() = now_ts;
            return Ok(());
        }
        let tz = stats::timezone(&a.preferences.timezone)?;
        let (start, end) = stats::bounds(range, a.preferences.recent_minutes, tz, now)?;
        let mut snapshot = old
            .filter(|v| v.range == range && v.timezone == a.preferences.timezone)
            .unwrap_or(UsageSnapshot {
                account_id: id.into(),
                generation,
                range,
                start: start.to_rfc3339(),
                end: end.to_rfc3339(),
                timezone: a.preferences.timezone.clone(),
                totals: None,
                balance: None,
                recent: None,
                latest: None,
                sync: SyncStatus::default(),
                demo: false,
            });
        snapshot.generation = generation;
        let mut errors: Vec<ApiError> = vec![];
        let mut recent_complete = true;
        let mut summary_success = false;
        if recent_due {
            let recent_start = now - Duration::minutes(i64::from(a.preferences.recent_minutes));
            match self.collect(&a, &rt, recent_start, now, None, None).await {
                Ok((rows, complete)) => {
                    snapshot.recent = Some(stats::aggregate(&rows, recent_start, now));
                    snapshot.sync.recent_synced_at = Some(Utc::now().to_rfc3339());
                    recent_complete = complete;
                    if let Some(first) = rows.first() {
                        snapshot.latest = Some(first.clone());
                    }
                    if range == UsageRange::Recent {
                        snapshot.totals = snapshot.recent.clone();
                        snapshot.start = recent_start.to_rfc3339();
                        snapshot.end = now.to_rfc3339();
                        snapshot.sync.usage_synced_at = snapshot.sync.recent_synced_at.clone();
                        snapshot.sync.complete = complete;
                        summary_success = true;
                    }
                }
                Err(e) => errors.push(e),
            }
            // Latest request remains visible even when there were no requests in the recent window.
            match self.page(&a, &rt, vec![], 1, 1).await {
                Ok((rows, _)) => {
                    snapshot.latest = rows.into_iter().next();
                }
                Err(e) => errors.push(e),
            }
            *rt.last_recent.lock().unwrap() = now_ts;
        }
        if summary_due {
            match self.get(&a, &rt, "/user/profile", &[]).await {
                Ok(profile) => match stats::decimal(&profile["balance"]) {
                    Ok(balance) => {
                        snapshot.balance = Some(balance.normalize().to_string());
                        snapshot.sync.balance_synced_at = Some(Utc::now().to_rfc3339());
                    }
                    Err(e) => errors.push(e.into()),
                },
                Err(e) => errors.push(e),
            }
            if range != UsageRange::Recent || !recent_due {
                match self.summary(&a, &rt, range, start, end).await {
                    Ok((totals, complete)) => {
                        snapshot.totals = Some(totals);
                        snapshot.start = start.to_rfc3339();
                        snapshot.end = end.to_rfc3339();
                        snapshot.sync.usage_synced_at = Some(Utc::now().to_rfc3339());
                        snapshot.sync.complete = complete;
                        summary_success = true;
                    }
                    Err(e) => errors.push(e),
                }
            }
            *rt.last_summary.lock().unwrap() = now_ts;
        }
        snapshot.sync.complete = snapshot.sync.complete && recent_complete;
        if errors.is_empty() {
            snapshot.sync.state = if snapshot.sync.complete {
                "synced"
            } else {
                "incomplete"
            }
            .into();
            snapshot.sync.message = (!snapshot.sync.complete)
                .then(|| "记录分页变化或达到读取上限，当前统计尚不完整。".into());
            *rt.failures.lock().unwrap() = 0;
            let mut retry_at = rt.retry_at.lock().unwrap();
            // A concurrent detail read may have established a newer cooldown.
            if *retry_at <= Utc::now().timestamp() {
                *retry_at = 0;
            }
        } else {
            let expired = errors.iter().any(|e| e.unauthorized);
            snapshot.sync.state = if expired {
                "needsLogin"
            } else if summary_success {
                "partial"
            } else {
                "stale"
            }
            .into();
            snapshot.sync.message = Some(errors[0].message.clone());
            let failure = {
                let mut f = rt.failures.lock().unwrap();
                *f = (*f + 1).min(6);
                *f
            };
            let retry = errors
                .iter()
                .filter_map(|e| e.retry_after)
                .max()
                .unwrap_or(5 * 2_u64.pow(failure));
            let mut retry_at = rt.retry_at.lock().unwrap();
            *retry_at = (*retry_at).max(Utc::now().timestamp() + retry as i64);
        }
        {
            let mut s = self.state.lock().unwrap();
            if s.runtime.get(id).is_some_and(|live| Arc::ptr_eq(live, &rt))
                && *rt.range.lock().unwrap() == range
            {
                if let Some(saved) = s.config.accounts.iter_mut().find(|v| v.summary.id == id) {
                    if saved.summary.preferences == a.preferences {
                        if saved.summary.needs_login {
                            snapshot.sync.state = "needsLogin".into();
                            snapshot.sync.message = Some("登录已过期，请重新登录该账号。".into());
                        } else if *rt.retry_at.lock().unwrap() > Utc::now().timestamp()
                            && errors.is_empty()
                        {
                            snapshot.sync.state = "stale".into();
                            snapshot.sync.message = Some("账号请求已暂缓，稍后自动重试。".into());
                        }
                        saved.snapshot = Some(snapshot);
                    } else {
                        *rt.last_summary.lock().unwrap() = 0;
                        *rt.last_recent.lock().unwrap() = 0;
                    }
                }
            } else {
                *rt.last_summary.lock().unwrap() = 0;
                *rt.last_recent.lock().unwrap() = 0;
            }
        }
        self.persist()?;
        Ok(())
    }
    /// Native host invokes one shared coordinator; skipped ticks do not queue after sleep.
    pub fn account_ids(&self) -> Vec<String> {
        let s = self.state.lock().unwrap();
        let mut ids = s
            .config
            .accounts
            .iter()
            .filter(|a| !a.summary.needs_login || a.summary.demo)
            .map(|a| a.summary.id.clone())
            .collect::<Vec<_>>();
        ids.sort_by_key(|id| Some(id) != s.config.current.as_ref());
        ids
    }
    pub async fn refresh_due(&self, id: String) -> bool {
        let before = {
            let s = self.state.lock().unwrap();
            s.config
                .accounts
                .iter()
                .find(|a| a.summary.id == id)
                .and_then(|a| serde_json::to_string(&a.snapshot).ok())
        };
        let _ = self.refresh_account(&id, false).await;
        let after = {
            let s = self.state.lock().unwrap();
            s.config
                .accounts
                .iter()
                .find(|a| a.summary.id == id)
                .and_then(|a| serde_json::to_string(&a.snapshot).ok())
        };
        before != after
    }
    pub async fn query_records(&self, query: RecordQuery) -> Result<RecordPage, String> {
        let (a, rt) = self.context(&query.account_id)?;
        let generation = self.state.lock().unwrap().generation;
        let page = query.page.max(1);
        let size = query.page_size.clamp(1, 100);
        let now = Utc::now();
        let (mut start, mut end) = stats::bounds(
            query.range,
            a.preferences.recent_minutes,
            stats::timezone(&a.preferences.timezone)?,
            now,
        )?;
        let (items, total, complete, message) = if a.demo {
            let rows = self
                .demo_records(&a, now)
                .into_iter()
                .filter(|r| {
                    stats::within(r, start, end)
                        && query.api_key_id.is_none_or(|id| r.api_key_id == id)
                        && query
                            .model
                            .as_ref()
                            .is_none_or(|m| m.is_empty() || r.model == *m)
                })
                .collect::<Vec<_>>();
            let total = rows.len() as u64;
            (
                rows.into_iter()
                    .skip(((page - 1) * size) as usize)
                    .take(size as usize)
                    .collect(),
                total,
                true,
                None,
            )
        } else if matches!(query.range, UsageRange::Week | UsageRange::Recent) {
            let result = self
                .window_records(&a, &rt, &query, page, size, now)
                .await
                .map_err(|e| e.message)?;
            start = result.start;
            end = result.end;
            (result.items, result.total, result.complete, result.message)
        } else {
            let mut params = Self::dates(&a, start, end).map_err(|e| e.message)?;
            Self::filters(&mut params, query.api_key_id, query.model.as_deref());
            let (items, total) = self
                .page(&a, &rt, params, page, size)
                .await
                .map_err(|e| e.message)?;
            (items, total, true, None)
        };
        Ok(RecordPage {
            account_id: a.id,
            generation,
            range: query.range,
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            timezone: a.preferences.timezone,
            items,
            total,
            page,
            page_size: size,
            complete,
            message,
        })
    }
    pub async fn analysis(&self, query: AnalysisQuery) -> Result<AnalysisResult, String> {
        let (a, rt) = self.context(&query.account_id)?;
        let now = Utc::now();
        let tz = stats::timezone(&a.preferences.timezone)?;
        let (start, _) = stats::bounds(query.range, a.preferences.recent_minutes, tz, now)?;
        let calendar_boundary = if matches!(query.range, UsageRange::Today | UsageRange::Month) {
            start.timestamp()
        } else {
            0
        };
        let key = format!(
            "{:?}|{:?}|{:?}|{}|{}|{}|{}",
            query.range,
            query.api_key_id,
            query.model,
            a.preferences.timezone,
            a.preferences.recent_minutes,
            query.include_keys,
            calendar_boundary
        );
        let mut cache = rt.analysis.lock().await;
        if let Some((cached_key, at, result)) = &*cache {
            if !query.force && *cached_key == key && now.timestamp() - at < 300 {
                let mut result = result.clone();
                result.generation = self.state.lock().unwrap().generation;
                return Ok(result);
            }
        }
        if !a.demo && matches!(query.range, UsageRange::Today | UsageRange::Month) {
            let tz = stats::timezone(&a.preferences.timezone)?;
            let (start, end) = stats::bounds(query.range, a.preferences.recent_minutes, tz, now)?;
            let mut params = Self::dates(&a, start, end).map_err(|e| e.message)?;
            Self::filters(&mut params, query.api_key_id, query.model.as_deref());
            params.push((
                "granularity".into(),
                if query.range == UsageRange::Today {
                    "hour"
                } else {
                    "day"
                }
                .into(),
            ));
            let trend_data = self
                .get(&a, &rt, "/usage/dashboard/trend", &params)
                .await
                .map_err(|e| e.message)?;
            let models_data = self
                .get(&a, &rt, "/usage/dashboard/models", &params)
                .await
                .map_err(|e| e.message)?;
            let parse_rows =
                |data: &Value, field: &str, label: &str| -> Result<Vec<AnalysisRow>, String> {
                    data[field]
                        .as_array()
                        .ok_or("站点图表格式不兼容")?
                        .iter()
                        .map(|v| {
                            Ok(AnalysisRow {
                                name: v[label].as_str().ok_or("站点图表缺少标签")?.into(),
                                totals: Totals::from_chart(v)?,
                            })
                        })
                        .collect()
                };
            let trend = parse_rows(&trend_data, "trend", "date")?;
            let models = parse_rows(&models_data, "models", "model")?;
            let mut keys = BTreeMap::<String, Totals>::new();
            let mut complete = true;
            if query.include_keys {
                let (records, covered) = self
                    .collect(
                        &a,
                        &rt,
                        start,
                        end,
                        query.api_key_id,
                        query.model.as_deref(),
                    )
                    .await
                    .map_err(|e| e.message)?;
                complete = covered;
                for r in records {
                    keys.entry(format!("{} · #{}", r.api_key_name, r.api_key_id))
                        .or_insert_with(Totals::zero)
                        .add(&r.totals);
                }
            }
            let result = AnalysisResult {
                account_id: a.id,
                generation: self.state.lock().unwrap().generation,
                range: query.range,
                start: start.to_rfc3339(),
                end: end.to_rfc3339(),
                timezone: a.preferences.timezone.clone(),
                trend,
                models,
                keys: keys
                    .into_iter()
                    .map(|(name, totals)| AnalysisRow { name, totals })
                    .collect(),
                complete,
                message: Some(
                    if complete {
                        "范围按所选时区筛选；趋势标签采用站点分桶时间，其时区尚未验证。"
                    } else {
                        "趋势标签采用站点分桶时间；Key 分析只覆盖已读取的最多 5,000 条记录。"
                    }
                    .into(),
                ),
                synced_at: Utc::now().to_rfc3339(),
            };
            *cache = Some((key, now.timestamp(), result.clone()));
            return Ok(result);
        }
        let tz = stats::timezone(&a.preferences.timezone)?;
        let (start, end) = stats::bounds(query.range, a.preferences.recent_minutes, tz, now)?;
        let (records, complete) = if a.demo {
            (
                self.demo_records(&a, now)
                    .into_iter()
                    .filter(|r| {
                        stats::within(r, start, end)
                            && query.api_key_id.is_none_or(|id| id == r.api_key_id)
                            && query
                                .model
                                .as_ref()
                                .is_none_or(|m| m.is_empty() || *m == r.model)
                    })
                    .collect(),
                true,
            )
        } else {
            self.collect(
                &a,
                &rt,
                start,
                end,
                query.api_key_id,
                query.model.as_deref(),
            )
            .await
            .map_err(|e| e.message)?
        };
        let mut trend = BTreeMap::<String, Totals>::new();
        let mut models = BTreeMap::<String, Totals>::new();
        let mut keys = BTreeMap::<String, Totals>::new();
        for r in records {
            let date = DateTime::parse_from_rfc3339(&r.created_at)
                .map_err(|_| "时间格式无效")?
                .with_timezone(&tz);
            let label = if query.range == UsageRange::Recent {
                date.format("%Y-%m-%d %H:%M").to_string()
            } else if query.range == UsageRange::Today {
                date.format("%Y-%m-%d %H:00").to_string()
            } else {
                date.format("%Y-%m-%d").to_string()
            };
            trend
                .entry(label)
                .or_insert_with(Totals::zero)
                .add(&r.totals);
            models
                .entry(r.model)
                .or_insert_with(Totals::zero)
                .add(&r.totals);
            keys.entry(format!("{} · #{}", r.api_key_name, r.api_key_id))
                .or_insert_with(Totals::zero)
                .add(&r.totals);
        }
        let rows = |map: BTreeMap<String, Totals>| {
            map.into_iter()
                .map(|(name, totals)| AnalysisRow { name, totals })
                .collect()
        };
        let result = AnalysisResult {
            account_id: a.id,
            generation: self.state.lock().unwrap().generation,
            range: query.range,
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            timezone: a.preferences.timezone.clone(),
            trend: rows(trend),
            models: rows(models),
            keys: rows(keys),
            complete,
            message: (!complete).then(|| {
                "分析最多读取 5,000 条记录；当前图表仅覆盖已读取记录，不代表完整范围。".into()
            }),
            synced_at: Utc::now().to_rfc3339(),
        };
        *cache = Some((key, now.timestamp(), result.clone()));
        Ok(result)
    }
    fn demo_records(&self, a: &AccountSummary, now: DateTime<Utc>) -> Vec<UsageRecord> {
        let mut rows = vec![];
        for i in 0..180 {
            let mut totals = Totals {
                cost: format!("0.{:04}", 100 + (i * 137) % 4000),
                requests: 1,
                input_tokens: 1200 + (i as u64) * 13,
                output_tokens: 560 + (i as u64) * 3,
                cache_read_tokens: 5400 + (i as u64) * 40,
                cache_creation_tokens: 800,
                total_tokens: 0,
                cache_rate: None,
            };
            totals.recalculate();
            rows.push(UsageRecord {
                id: 10000 - i,
                created_at: (now - Duration::seconds(if i < 5 { 30 + i * 50 } else { i * 3600 }))
                    .to_rfc3339(),
                model: if i % 3 == 0 {
                    "claude-sonnet-4-5"
                } else {
                    "gpt-5"
                }
                .into(),
                api_key_id: if i % 2 == 0 { 1 } else { 2 },
                api_key_name: if i % 2 == 0 {
                    "日常使用"
                } else {
                    "开发测试"
                }
                .into(),
                totals,
                duration_ms: Some(1200.0 + (i as f64) * 3.0),
                stream: true,
            });
        }
        let _ = a;
        rows
    }
    fn demo_snapshot(
        &self,
        a: &AccountSummary,
        range: UsageRange,
        generation: u64,
    ) -> Result<UsageSnapshot, String> {
        let now = Utc::now();
        let (start, end) = stats::bounds(
            range,
            a.preferences.recent_minutes,
            stats::timezone(&a.preferences.timezone)?,
            now,
        )?;
        let rows = self.demo_records(a, now);
        Ok(UsageSnapshot {
            account_id: a.id.clone(),
            generation,
            range,
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            timezone: a.preferences.timezone.clone(),
            totals: Some(stats::aggregate(&rows, start, end)),
            balance: Some(if a.user_id == 1 { "86.50" } else { "128.75" }.into()),
            recent: Some(stats::aggregate(
                &rows,
                now - Duration::minutes(i64::from(a.preferences.recent_minutes)),
                end,
            )),
            latest: rows.first().cloned(),
            sync: SyncStatus {
                state: "synced".into(),
                message: Some("演示数据，未连接真实站点。".into()),
                usage_synced_at: Some(now.to_rfc3339()),
                balance_synced_at: Some(now.to_rfc3339()),
                recent_synced_at: Some(now.to_rfc3339()),
                complete: true,
            },
            demo: true,
        })
    }
    pub async fn enable_demo(&self) -> Result<Bootstrap, String> {
        let mut accounts = vec![];
        for (id, alias, role, range, fields) in [
            (
                1,
                "自建站 · 演示",
                "admin",
                UsageRange::Today,
                vec!["cost", "requests", "tokens", "cache"],
            ),
            (
                2,
                "上游套餐 · 演示",
                "user",
                UsageRange::Week,
                vec!["balance", "cost", "tokens", "cache"],
            ),
        ] {
            let summary = AccountSummary {
                id: format!("demo:{id}"),
                site: format!("https://demo-{id}.example.com"),
                email: "demo@example.com".into(),
                role: role.into(),
                user_id: id,
                preferences: AccountPreferences {
                    alias: alias.into(),
                    default_range: range,
                    metrics: fields.into_iter().map(str::to_owned).collect(),
                    ..AccountPreferences::default()
                },
                needs_login: false,
                demo: true,
            };
            accounts.push(summary);
        }
        {
            let mut s = self.state.lock().unwrap();
            for summary in accounts {
                if !s.config.accounts.iter().any(|a| a.summary.id == summary.id) {
                    s.runtime.insert(
                        summary.id.clone(),
                        Arc::new(Runtime::new(None, summary.preferences.default_range)),
                    );
                    s.config.accounts.push(SavedAccount {
                        summary,
                        encrypted_session: None,
                        snapshot: None,
                    });
                }
            }
            s.config.current = Some("demo:1".into());
            s.generation += 1;
        }
        self.refresh(Some("demo:1".into()), None, true).await
    }
}
