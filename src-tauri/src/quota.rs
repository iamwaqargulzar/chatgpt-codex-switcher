use crate::error::{AppError, R};
use crate::models::{
    DayPoint, NameValue, RateBucket, ResetCredits, ResetCredit, SoftHard, TokenSet, UsageSnapshot,
    UsageStats,
};
use chrono::{Duration, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;

const BASE: &str = "https://chatgpt.com/backend-api";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";

pub enum QuotaError {
    Unauthorized,
    Other(String),
}

impl From<QuotaError> for AppError {
    fn from(e: QuotaError) -> Self {
        match e {
            QuotaError::Unauthorized => AppError::Msg("session expired (401)".into()),
            QuotaError::Other(s) => AppError::Msg(s),
        }
    }
}

/// Everything CodexDesk talks to is hard-coded in this file: the ChatGPT
/// backend API and the OAuth token endpoint. Nothing else is ever contacted.
#[derive(Clone)]
pub struct QuotaClient {
    http: reqwest::Client,
}

impl QuotaClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("static reqwest client"),
        }
    }

    fn headers(&self, tokens: &TokenSet) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, HeaderValue::from_static(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        ));
        h.insert(ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
        h.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        h.insert(ORIGIN, HeaderValue::from_static("https://chatgpt.com"));
        h.insert(REFERER, HeaderValue::from_static("https://chatgpt.com/"));
        let bearer = tokens
            .id_token
            .clone()
            .unwrap_or_else(|| tokens.access_token.clone());
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {bearer}")) {
            h.insert(AUTHORIZATION, v);
        }
        h
    }

    async fn get_json(&self, url: &str, tokens: &TokenSet) -> Result<Value, QuotaError> {
        let resp = self
            .http
            .get(url)
            .headers(self.headers(tokens))
            .send()
            .await
            .map_err(|e| QuotaError::Other(e.to_string()))?;
        match resp.status().as_u16() {
            200 => resp
                .json::<Value>()
                .await
                .map_err(|e| QuotaError::Other(e.to_string())),
            401 | 403 => Err(QuotaError::Unauthorized),
            other => Err(QuotaError::Other(format!("backend responded {other}"))),
        }
    }

    /// Resolve the ChatGPT account id for a token set.
    pub async fn check_account(&self, tokens: &TokenSet) -> R<Option<String>> {
        if let Some(id) = &tokens.account_id {
            if !id.is_empty() {
                return Ok(Some(id.clone()));
            }
        }
        let v = self
            .get_json(
                &format!("{BASE}/accounts/check/v4-2023-04-27"),
                tokens,
            )
            .await?;
        let id = dig_string(&v, "account_id").or_else(|| {
            tokens
                .id_token
                .as_deref()
                .and_then(|t| jwt_claim(t, "https://api.openai.com/v1/accounts"))
                .or_else(|| tokens.id_token.as_deref().and_then(|t| jwt_claim(t, "account_id")))
                .or_else(|| tokens.id_token.as_deref().and_then(|t| jwt_claim(t, "sub")))
        });
        Ok(id)
    }

    /// Swap a refresh token for fresh tokens.
    pub async fn refresh_tokens(&self, tokens: &TokenSet) -> R<TokenSet> {
        let refresh = tokens
            .refresh_token
            .clone()
            .ok_or_else(|| AppError::Msg("no refresh token available".into()))?;
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
            ("client_id", crate::authz::CLIENT_ID),
        ];
        let v = self.token_request(&form).await?;
        if v.get("error").is_some() {
            return Err(AppError::Msg("token refresh rejected".into()));
        }
        Ok(merge_token_response(tokens, &v))
    }

    /// POST a form to the official OAuth token endpoint.
    pub async fn token_request(&self, form: &[(&str, &str)]) -> R<serde_json::Value> {
        let resp = self
            .http
            .post(TOKEN_ENDPOINT)
            .header(USER_AGENT, "codexdesk/0.1")
            .header(ACCEPT, "application/json")
            .form(form)
            .send()
            .await?;
        Ok(resp.json().await?)
    }

    /// Lightweight call used by warm-ups: proves the session still works.
    pub async fn health_check(&self, tokens: &TokenSet) -> R<()> {
        self.get_json(&format!("{BASE}/me"), tokens).await?;
        Ok(())
    }

    /// Warm-up probe for API-key accounts: list models once.
    pub async fn probe_api_key(&self, key: &str) -> R<()> {
        let resp = self
            .http
            .get("https://api.openai.com/v1/models")
            .header(AUTHORIZATION, format!("Bearer {key}"))
            .header(USER_AGENT, "codexdesk/0.1")
            .send()
            .await?;
        match resp.status().as_u16() {
            200 => Ok(()),
            other => Err(AppError::Msg(format!("API responded {other}"))),
        }
    }

    /// Pull every quota signal for an account. With `with_stats` false the
    /// heavier daily-usage call is skipped and previous stats are kept by the
    /// caller.
    pub async fn fetch_snapshot(
        &self,
        tokens: &TokenSet,
        with_stats: bool,
    ) -> Result<UsageSnapshot, QuotaError> {
        let account_id = self
            .check_account(tokens)
            .await
            .map_err(|e| QuotaError::Other(e.to_string()))?
            .unwrap_or_default();
        if account_id.is_empty() {
            return Err(QuotaError::Other(
                "could not resolve the ChatGPT account id".into(),
            ));
        }

        let mut snap = UsageSnapshot {
            fetched_at: Utc::now().timestamp(),
            account_id: Some(account_id.clone()),
            ..Default::default()
        };

        // Profile + plan + subscription expiry.
        let me = self.get_json(&format!("{BASE}/me"), tokens).await?;
        snap.plan = dig_string(&me, "entitled_plan")
            .or_else(|| dig_string(&me, "plan_type"));
        snap.subscription_expires_at =
            dig_i64(&me, "subscription_expires_at").or_else(|| dig_i64(&me, "expires_at"));

        // Session / weekly limits in seconds.
        let usage = self
            .get_json(&format!("{BASE}/accounts/{account_id}/usage"), tokens)
            .await?;
        snap.system_hard_limit_usd =
            dig_f64(&usage, "system_hard_limit_usd").or_else(|| dig_f64(&usage, "hard_limit_usd"));
        let capped = usage.get("capped").and_then(|c| c.get("daily_usage_limit_seconds"));
        let free = usage.get("free_tier").and_then(|c| c.get("daily_usage_limit_seconds"));
        let soft_hard = |v: Option<&Value>| -> Option<SoftHard> {
            v.map(|s| SoftHard {
                soft: s.get("soft").and_then(Value::as_i64),
                hard: s.get("hard").and_then(Value::as_i64),
            })
        };
        snap.session_seconds = soft_hard(capped).or_else(|| soft_hard(free));

        // Token buckets: pick the most constrained 5h and weekly windows.
        let rl = self
            .get_json(&format!("{BASE}/accounts/{account_id}/rate_limits"), tokens)
            .await?;
        snap.system_hard_limit_usd =
            snap.system_hard_limit_usd.or_else(|| dig_f64(&rl, "system_hard_limit_usd"));
        if let Some(buckets) = rl.get("buckets").and_then(Value::as_object) {
            let mut session: Vec<RateBucket> = Vec::new();
            let mut weekly: Vec<RateBucket> = Vec::new();
            for (name, b) in buckets {
                let Some(reset_at) = b.get("reset_at").and_then(Value::as_i64) else {
                    continue;
                };
                let window = b
                    .get("window")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let bucket = RateBucket {
                    name: name.clone(),
                    window: window.clone(),
                    reset_at,
                    maximum_tokens: b.get("maximum_tokens").and_then(Value::as_f64).unwrap_or(0.0),
                    remaining_tokens: b
                        .get("remaining_tokens")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    used_tokens: b.get("used_tokens").and_then(Value::as_f64).unwrap_or(0.0),
                };
                let lname = name.to_lowercase();
                if window == "weekly" || lname.contains("weekly") {
                    weekly.push(bucket);
                } else if window == "5h" || lname.contains("codex") {
                    session.push(bucket);
                }
            }
            snap.session = pick_most_constrained(session);
            snap.weekly = pick_most_constrained(weekly);
        }

        // Manual reset credits (the endpoint only exists for eligible plans).
        if let Ok(v) = self
            .get_json(
                &format!("{BASE}/accounts/{account_id}/manual_reset_credits"),
                tokens,
            )
            .await
        {
            let resets = v
                .get("resets")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|r| {
                            Some(ResetCredit {
                                id: r
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                expires_at: r.get("expires_at").and_then(Value::as_i64)?,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            snap.reset_credits = Some(ResetCredits {
                available_count: v.get("available_count").and_then(Value::as_i64).unwrap_or(0),
                resets,
            });
        }

        // Full usage history (skipped on routine refreshes).
        if with_stats {
            let start = (Utc::now() - Duration::days(2500)).format("%Y-%m-%d");
            let end = Utc::now().format("%Y-%m-%d");
            let url = format!(
                "{BASE}/accounts/{account_id}/daily_usage?start_date={start}&end_date={end}"
            );
            if let Ok(v) = self.get_json(&url, tokens).await {
                snap.stats = compute_stats(&v);
            }
        }

        Ok(snap)
    }
}

/// Most-constrained = lowest remaining/maximum fraction (excluding zero max).
fn pick_most_constrained(buckets: Vec<RateBucket>) -> Option<RateBucket> {
    buckets.into_iter().min_by(|a, b| {
        let ra = if a.maximum_tokens > 0.0 {
            a.remaining_tokens / a.maximum_tokens
        } else {
            1.0
        };
        let rb = if b.maximum_tokens > 0.0 {
            b.remaining_tokens / b.maximum_tokens
        } else {
            1.0
        };
        ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal)
    })
}

// ---------- daily usage stats ----------

fn compute_stats(daily: &Value) -> Option<UsageStats> {
    let mut days: Vec<DayPoint> = daily
        .get("daily_usage")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|d| {
            let date = d.get("date").and_then(Value::as_str)?;
            let tokens = sum_tokens(d);
            Some(DayPoint {
                date: date.to_string(),
                tokens,
            })
        })
        .collect();
    days.sort_by(|a, b| a.date.cmp(&b.date));

    let last30: Vec<&DayPoint> = days.iter().rev().take(30).collect();
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let today_tokens = last30
        .iter()
        .find(|d| d.date == today)
        .map(|d| d.tokens)
        .unwrap_or(0.0);
    let last7_tokens: f64 = last30.iter().take(7).map(|d| d.tokens).sum();
    let last30_tokens: f64 = last30.iter().map(|d| d.tokens).sum();
    let lifetime_tokens: f64 = days.iter().map(|d| d.tokens).sum();

    // Streaks over the whole series.
    let (mut current, mut longest, mut run) = (0u32, 0u32, 0u32);
    for d in days.iter() {
        if d.tokens > 0.0 {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let has_activity = |date: &str| -> bool {
        days.iter()
            .rev()
            .find(|d| d.date == date)
            .map(|d| d.tokens > 0.0)
            .unwrap_or(false)
    };
    if has_activity(&today) || has_activity(&yesterday()) {
        let start_day = if has_activity(&today) {
            today.clone()
        } else {
            yesterday()
        };
        current = streak_from(&days, &start_day);
    }

    let busiest = days.iter().max_by(|a, b| a.tokens.total_cmp(&b.tokens)).cloned();

    // Per-integration aggregation, wherever the backend reports it.
    let mut integrations: Vec<NameValue> = Vec::new();
    if let Some(arr) = daily.get("daily_usage").and_then(Value::as_array) {
        for d in arr {
            for key in ["top_integrations", "integrations", "breakdown"] {
                if let Some(list) = d.get(key).and_then(Value::as_array) {
                    for item in list {
                        let name = item
                            .get("name")
                            .or_else(|| item.get("title"))
                            .or_else(|| item.get("integration"))
                            .and_then(Value::as_str);
                        let val = item
                            .get("tokens")
                            .or_else(|| item.get("value"))
                            .or_else(|| item.get("usage_seconds"))
                            .and_then(Value::as_f64);
                        if let (Some(name), Some(val)) = (name, val) {
                            match integrations.iter_mut().find(|i| i.name == name) {
                                Some(e) => e.tokens += val,
                                None => integrations.push(NameValue {
                                    name: name.to_string(),
                                    tokens: val,
                                }),
                            }
                        }
                    }
                }
            }
        }
    }
    integrations.sort_by(|a, b| b.tokens.total_cmp(&a.tokens));
    integrations.truncate(10);

    Some(UsageStats {
        lifetime_tokens,
        today_tokens,
        last7_tokens,
        last30_tokens,
        current_streak_days: current,
        longest_streak_days: longest,
        busiest_day: busiest.map(|d| DayPoint {
            date: d.date.clone(),
            tokens: d.tokens,
        }),
        daily: last30.into_iter().cloned().collect(),
        integrations,
    })
}

fn yesterday() -> String {
    (Utc::now() - Duration::days(1)).format("%Y-%m-%d").to_string()
}

fn streak_from(days: &[DayPoint], start_day: &str) -> u32 {
    let mut n = 0;
    let mut day = chrono::NaiveDate::parse_from_str(start_day, "%Y-%m-%d").ok();
    while let Some(d) = day {
        let key = d.format("%Y-%m-%d").to_string();
        let active = days
            .iter()
            .rev()
            .find(|p| p.date == key)
            .map(|p| p.tokens > 0.0)
            .unwrap_or(false);
        if !active {
            break;
        }
        n += 1;
        day = d.pred_opt();
    }
    n
}

/// Sum every numeric leaf whose key mentions tokens or usage seconds.
fn sum_tokens(v: &Value) -> f64 {
    let mut total = 0.0;
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                let lk = k.to_lowercase();
                match val {
                    Value::Number(_) if lk.contains("token") || lk == "usage_seconds" => {
                        total += val.as_f64().unwrap_or(0.0);
                    }
                    _ => total += sum_tokens(val),
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                total += sum_tokens(item);
            }
        }
        _ => {}
    }
    total
}

// ---------- generic JSON diggers ----------

pub fn dig_string(v: &Value, key: &str) -> Option<String> {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == key {
                    if let Some(s) = val.as_str() {
                        return Some(s.to_string());
                    }
                    if let Some(s) = val.get("id").or_else(|| val.get("value")) {
                        if let Some(s) = s.as_str() {
                            return Some(s.to_string());
                        }
                    }
                }
                if let Some(found) = dig_string(val, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|item| dig_string(item, key)),
        _ => None,
    }
}

pub fn dig_i64(v: &Value, key: &str) -> Option<i64> {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == key {
                    return val.as_i64();
                }
                if let Some(found) = dig_i64(val, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|item| dig_i64(item, key)),
        _ => None,
    }
}

pub fn dig_f64(v: &Value, key: &str) -> Option<f64> {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == key {
                    return val.as_f64().or_else(|| val.as_i64().map(|i| i as f64));
                }
                if let Some(found) = dig_f64(val, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|item| dig_f64(item, key)),
        _ => None,
    }
}

/// Decode a claim from a JWT payload without verifying the signature (the
/// token came from the official token endpoint over TLS).
pub fn jwt_claim(id_token: &str, claim: &str) -> Option<String> {
    let part = id_token.split('.').nth(1)?;
    let padded = {
        let mut p = part.to_string();
        while p.len() % 4 != 0 {
            p.push('=');
        }
        p
    };
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let bytes = URL_SAFE_NO_PAD.decode(padded).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    dig_string(&v, claim)
}

fn merge_token_response(old: &TokenSet, v: &Value) -> TokenSet {
    let mut t = merge_tokens_response(v);
    if t.access_token.is_empty() {
        t.access_token = old.access_token.clone();
    }
    if t.id_token.is_none() {
        t.id_token = old.id_token.clone();
    }
    if t.refresh_token.is_none() {
        t.refresh_token = old.refresh_token.clone();
    }
    if t.account_id.is_none() {
        t.account_id = old.account_id.clone();
    }
    t
}

/// Build a fresh TokenSet from a token-endpoint JSON response.
pub fn merge_tokens_response(v: &Value) -> TokenSet {
    let expires_in = v.get("expires_in").and_then(Value::as_i64);
    TokenSet {
        access_token: v
            .get("access_token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        id_token: v.get("id_token").and_then(Value::as_str).map(|s| s.to_string()),
        refresh_token: v
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(|s| s.to_string()),
        account_id: v
            .get("account_id")
            .and_then(Value::as_str)
            .map(|s| s.to_string()),
        expires_at: expires_in.map(|s| Utc::now().timestamp() + s),
    }
}
