use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UsageRange {
    #[default]
    Today,
    Week,
    Month,
    Recent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountPreferences {
    pub alias: String,
    pub default_range: UsageRange,
    pub metrics: Vec<String>,
    pub recent_minutes: u32,
    pub timezone: String,
}
impl Default for AccountPreferences {
    fn default() -> Self {
        Self {
            alias: String::new(),
            default_range: UsageRange::Today,
            metrics: vec![
                "cost".into(),
                "requests".into(),
                "tokens".into(),
                "cache".into(),
            ],
            recent_minutes: 5,
            timezone: "Asia/Shanghai".into(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AppearanceTheme {
    #[default]
    Light,
    Dark,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub theme: AppearanceTheme,
    pub opacity: f64,
    pub always_on_top: bool,
    pub recent_refresh_seconds: u64,
    pub summary_refresh_seconds: u64,
    pub background_refresh_seconds: u64,
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: AppearanceTheme::Light,
            opacity: 1.0,
            always_on_top: true,
            recent_refresh_seconds: 10,
            summary_refresh_seconds: 30,
            background_refresh_seconds: 120,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct AppSettingsPatch {
    pub theme: Option<AppearanceTheme>,
    pub opacity: Option<f64>,
    pub always_on_top: Option<bool>,
    pub recent_refresh_seconds: Option<u64>,
    pub summary_refresh_seconds: Option<u64>,
    pub background_refresh_seconds: Option<u64>,
}
impl From<AppSettings> for AppSettingsPatch {
    fn from(settings: AppSettings) -> Self {
        Self {
            theme: Some(settings.theme),
            opacity: Some(settings.opacity),
            always_on_top: Some(settings.always_on_top),
            recent_refresh_seconds: Some(settings.recent_refresh_seconds),
            summary_refresh_seconds: Some(settings.summary_refresh_seconds),
            background_refresh_seconds: Some(settings.background_refresh_seconds),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub site: String,
    pub email: String,
    pub role: String,
    pub user_id: i64,
    pub preferences: AccountPreferences,
    pub needs_login: bool,
    pub demo: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub cost: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
    pub cache_rate: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: i64,
    pub created_at: String,
    pub model: String,
    pub api_key_id: i64,
    pub api_key_name: String,
    pub totals: Totals,
    pub duration_ms: Option<f64>,
    pub stream: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSyncStatus {
    pub state: String,
    pub message: Option<String>,
    pub synced_at: Option<String>,
    pub complete: bool,
}
impl Default for DataSyncStatus {
    fn default() -> Self {
        Self {
            state: "loading".into(),
            message: None,
            synced_at: None,
            complete: false,
        }
    }
}
impl DataSyncStatus {
    pub fn received(complete: bool, message: Option<String>) -> Self {
        Self {
            state: if complete { "synced" } else { "incomplete" }.into(),
            message,
            synced_at: Some(chrono::Utc::now().to_rfc3339()),
            complete,
        }
    }
    pub fn unavailable(&mut self, state: &str, message: &str) {
        self.state = state.into();
        self.message = Some(message.into());
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub state: String,
    pub message: Option<String>,
    pub usage_synced_at: Option<String>,
    pub balance_synced_at: Option<String>,
    pub recent_synced_at: Option<String>,
    pub complete: bool,
    #[serde(default)]
    pub usage: DataSyncStatus,
    #[serde(default)]
    pub balance: DataSyncStatus,
    #[serde(default)]
    pub recent: DataSyncStatus,
    #[serde(default)]
    pub latest: DataSyncStatus,
}
impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            state: "loading".into(),
            message: None,
            usage_synced_at: None,
            balance_synced_at: None,
            recent_synced_at: None,
            complete: false,
            usage: DataSyncStatus::default(),
            balance: DataSyncStatus::default(),
            recent: DataSyncStatus::default(),
            latest: DataSyncStatus::default(),
        }
    }
}
impl SyncStatus {
    pub fn unavailable(&mut self, state: &str, message: &str) {
        for part in [
            &mut self.usage,
            &mut self.balance,
            &mut self.recent,
            &mut self.latest,
        ] {
            part.unavailable(state, message);
        }
        self.state = state.into();
        self.message = Some(message.into());
    }
    pub fn update_overall(&mut self) {
        self.complete = self.usage.complete;
        self.usage_synced_at = self.usage.synced_at.clone();
        self.balance_synced_at = self.balance.synced_at.clone();
        self.recent_synced_at = self.recent.synced_at.clone();
        let parts = [
            ("当前范围", &self.usage),
            ("余额", &self.balance),
            ("最近窗口", &self.recent),
            ("最近一笔", &self.latest),
        ];
        if let Some((_, part)) = parts.iter().find(|(_, part)| part.state == "needsLogin") {
            self.state = "needsLogin".into();
            self.message = part.message.clone();
        } else if self.usage.state != "synced" {
            self.state = self.usage.state.clone();
            self.message = self.usage.message.clone();
        } else if let Some((label, part)) = parts.iter().find(|(_, part)| part.state != "synced") {
            self.state = "partial".into();
            self.message = Some(format!(
                "{label}：{}",
                part.message.as_deref().unwrap_or("等待同步。")
            ));
        } else {
            self.state = "synced".into();
            self.message = None;
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub account_id: String,
    pub generation: u64,
    pub range: UsageRange,
    pub start: String,
    pub end: String,
    pub timezone: String,
    pub totals: Option<Totals>,
    pub balance: Option<String>,
    pub recent: Option<Totals>,
    pub latest: Option<UsageRecord>,
    pub sync: SyncStatus,
    pub demo: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub accounts: Vec<AccountSummary>,
    pub current_account_id: Option<String>,
    pub selected_range: UsageRange,
    pub settings: AppSettings,
    pub snapshot: Option<UsageSnapshot>,
    pub generation: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginInput {
    pub site: String,
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub remember: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginOutcome {
    pub status: String,
    pub challenge_id: Option<String>,
    pub masked_email: Option<String>,
    pub state: Option<Bootstrap>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordQuery {
    pub account_id: String,
    pub range: UsageRange,
    #[serde(default = "one")]
    pub page: u32,
    #[serde(default = "page_size")]
    pub page_size: u32,
    pub api_key_id: Option<i64>,
    pub model: Option<String>,
}
fn one() -> u32 {
    1
}
fn page_size() -> u32 {
    20
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordPage {
    pub account_id: String,
    pub generation: u64,
    pub range: UsageRange,
    pub start: String,
    pub end: String,
    pub timezone: String,
    pub items: Vec<UsageRecord>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub complete: bool,
    pub message: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisQuery {
    pub account_id: String,
    pub range: UsageRange,
    pub api_key_id: Option<i64>,
    pub model: Option<String>,
    #[serde(default)]
    pub include_keys: bool,
    #[serde(default)]
    pub force: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRow {
    pub name: String,
    pub totals: Totals,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket_start: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendMetadata {
    pub source: String,
    pub granularity: String,
    pub timezone: Option<String>,
    pub missing_buckets: String,
    pub complete: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub account_id: String,
    pub generation: u64,
    pub range: UsageRange,
    pub start: String,
    pub end: String,
    pub timezone: String,
    pub trend: Vec<AnalysisRow>,
    pub trend_meta: TrendMetadata,
    pub models: Vec<AnalysisRow>,
    pub keys: Vec<AnalysisRow>,
    pub complete: bool,
    pub message: Option<String>,
    pub synced_at: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptionsQuery {
    pub account_id: String,
    pub range: UsageRange,
    #[serde(default)]
    pub force: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyOption {
    pub id: i64,
    pub name: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions {
    pub account_id: String,
    pub generation: u64,
    pub range: UsageRange,
    pub start: String,
    pub end: String,
    pub timezone: String,
    pub models: Vec<String>,
    pub keys: Vec<KeyOption>,
    pub models_complete: bool,
    pub keys_complete: bool,
    pub message: Option<String>,
    pub synced_at: String,
}
