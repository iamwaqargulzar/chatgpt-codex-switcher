use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------- Accounts ----------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    Chatgpt,
    Apikey,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenSet {
    pub access_token: String,
    pub id_token: Option<String>,
    pub refresh_token: Option<String>,
    pub account_id: Option<String>,
    /// Unix seconds; None when the server did not tell us.
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthBundle {
    pub openai_api_key: Option<String>,
    pub tokens: Option<TokenSet>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WarmupConfig {
    pub enabled: bool,
    pub auto_after_reset: bool,
    /// "HH:MM" local times.
    pub timed_at: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub kind: AuthKind,
    pub profile: Option<String>,
    pub added_at: i64,
    pub last_used_at: Option<i64>,
    pub warmup: WarmupConfig,
    pub auth: AuthBundle,
}

/// What the frontend is allowed to see — credentials are stripped.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPublic {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub kind: AuthKind,
    pub profile: Option<String>,
    pub added_at: i64,
    pub last_used_at: Option<i64>,
    pub warmup: WarmupConfig,
    pub active: bool,
    pub plan: Option<String>,
    pub subscription_expires_at: Option<i64>,
}

impl Account {
    pub fn public(&self, active: bool, plan: Option<String>, sub_exp: Option<i64>) -> AccountPublic {
        AccountPublic {
            id: self.id.clone(),
            name: self.name.clone(),
            email: self.email.clone(),
            kind: self.kind.clone(),
            profile: self.profile.clone(),
            added_at: self.added_at,
            last_used_at: self.last_used_at,
            warmup: self.warmup.clone(),
            active,
            plan,
            subscription_expires_at: sub_exp,
        }
    }
}

// ---------- Quota ----------

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RateBucket {
    pub name: String,
    pub window: String,
    pub reset_at: i64,
    pub maximum_tokens: f64,
    pub remaining_tokens: f64,
    pub used_tokens: f64,
    /// "pct" when the backend reports percentages, "tokens" otherwise.
    pub unit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SoftHard {
    pub soft: Option<i64>,
    pub hard: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredit {
    pub id: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredits {
    pub available_count: i64,
    pub resets: Vec<ResetCredit>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayPoint {
    pub date: String,
    pub tokens: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameValue {
    pub name: String,
    pub tokens: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    pub lifetime_tokens: f64,
    pub today_tokens: f64,
    pub last7_tokens: f64,
    pub last30_tokens: f64,
    pub current_streak_days: u32,
    pub longest_streak_days: u32,
    pub busiest_day: Option<DayPoint>,
    pub daily: Vec<DayPoint>,
    pub integrations: Vec<NameValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub fetched_at: i64,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub subscription_expires_at: Option<i64>,
    pub system_hard_limit_usd: Option<f64>,
    pub session: Option<RateBucket>,
    pub weekly: Option<RateBucket>,
    pub session_seconds: Option<SoftHard>,
    pub weekly_seconds: Option<SoftHard>,
    pub reset_credits: Option<ResetCredits>,
    pub stats: Option<UsageStats>,
}

pub type SnapshotMap = HashMap<String, UsageSnapshot>;

// ---------- Logs ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub ts: i64,
    pub account_id: String,
    pub account_name: String,
    pub action: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub ts: i64,
    pub kind: String,
    pub account_id: Option<String>,
    pub detail: String,
}

// ---------- System ----------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: i32,
    pub name: String,
    pub cmdline: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    pub switched: bool,
    pub blocked: Vec<ProcessInfo>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub cwd: String,
    pub created_at: i64,
    pub title: String,
    pub message_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    pub name: String,
    pub is_base: bool,
    pub content: Option<String>,
}
