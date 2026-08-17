use crate::error::{AppError, R};
use crate::models::{
    DayPoint, NameValue, RateBucket, ResetCredits, ResetCredit, TokenSet, UsageSnapshot,
    UsageStats,
};
use chrono::{Duration, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;

const BASE: &str = "https://chatgpt.com/backend-api";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";

#[derive(Debug)]
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
    gate: Option<crate::gate::Gate>,
}

impl QuotaClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("static reqwest client"),
            gate: None,
        }
    }

    pub fn with_gate(gate: crate::gate::Gate) -> Self {
        let mut c = Self::new();
        c.gate = Some(gate);
        c
    }

    /// Headers for account-scoped backend calls. The access token
    /// authenticates; the id_token is only used by the check endpoint.
    fn headers(&self, tokens: &TokenSet) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, HeaderValue::from_static(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        ));
        h.insert(ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
        h.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        h.insert(ORIGIN, HeaderValue::from_static("https://chatgpt.com"));
        h.insert(REFERER, HeaderValue::from_static("https://chatgpt.com/"));
        let bearer = if tokens.access_token.is_empty() {
            tokens.id_token.clone().unwrap_or_default()
        } else {
            tokens.access_token.clone()
        };
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {bearer}")) {
            h.insert(AUTHORIZATION, v);
        }
        h
    }

    /// The account-check endpoint authenticates with the id_token.
    fn id_headers(&self, tokens: &TokenSet) -> HeaderMap {
        let mut h = self.headers(tokens);
        if let Some(id_token) = &tokens.id_token {
            if let Ok(v) = HeaderValue::from_str(&format!("Bearer {id_token}")) {
                h.insert(AUTHORIZATION, v);
            }
        }
        h
    }

    /// Direct request; returns (status, body) without interpreting it.
    pub async fn raw(&self, url: &str, tokens: &TokenSet) -> Result<(u16, String), QuotaError> {
        let resp = self
            .http
            .get(url)
            .headers(self.headers(tokens))
            .send()
            .await
            .map_err(|e| QuotaError::Other(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| QuotaError::Other(e.to_string()))?;
        Ok((status, body))
    }

    /// Fetch JSON from the ChatGPT backend. Cloudflare 403s fall back to the
    /// hidden webview gate, which carries a real challenge clearance.
    async fn get_json(&self, url: &str, tokens: &TokenSet) -> Result<Value, QuotaError> {
        let (status, body) = self.raw(url, tokens).await?;
        #[cfg(debug_assertions)]
        eprintln!("[quota] direct {status} <- {url}");
        if status == 200 {
            return serde_json::from_str(&body).map_err(|e| QuotaError::Other(e.to_string()));
        }
        if status == 401 {
            return Err(QuotaError::Unauthorized);
        }

        // Anything else (403 challenge pages, 5xx) gets one gate attempt.
        if let Some(gate) = &self.gate {
            let bearer = tokens
                .id_token
                .clone()
                .unwrap_or_else(|| tokens.access_token.clone());
            let (gstatus, gbody) = gate
                .fetch("GET", url, &bearer, None)
                .await
                .map_err(QuotaError::Other)?;
            if gstatus == 200 {
                return serde_json::from_str(&gbody)
                    .map_err(|e| QuotaError::Other(e.to_string()));
            }
            if gstatus == 401 {
                return Err(QuotaError::Unauthorized);
            }
            let preview = gbody.chars().take(120).collect::<String>();
            return Err(QuotaError::Other(format!(
                "backend responded {gstatus}: {preview}"
            )));
        }

        Err(QuotaError::Other(format!(
            "backend responded {status} (blocked by Cloudflare — retry in a moment)"
        )))
    }

    /// Resolve the ChatGPT account id for a token set.
    pub async fn check_account(&self, tokens: &TokenSet) -> R<Option<String>> {
        if let Some(id) = &tokens.account_id {
            if !id.is_empty() {
                return Ok(Some(id.clone()));
            }
        }
        let resp = self
            .http
            .get(format!("{BASE}/accounts/check/v4-2023-04-27"))
            .headers(self.id_headers(tokens))
            .send()
            .await?;
        let v: Value = resp.json().await?;
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

    /// Diagnostics helper (CODEXDESK_PROBE): fetch a URL directly and via
    /// the gate, printing status lines to stderr. When CODEXDESK_PROBE_METHOD
    /// is POST, CODEXDESK_PROBE_BODY is sent through the gate only.
    pub async fn probe(&self, url: &str, tokens: &TokenSet) {
        let method = std::env::var("CODEXDESK_PROBE_METHOD").unwrap_or_else(|_| "GET".into());
        let body = std::env::var("CODEXDESK_PROBE_BODY").ok();
        if method == "POST" {
            if let Some(gate) = &self.gate {
                let bearer = tokens
                    .id_token
                    .clone()
                    .unwrap_or_else(|| tokens.access_token.clone());
                match gate.fetch("POST", url, &bearer, body.as_deref()).await {
                    Ok((s, b)) => {
                        eprintln!("[probe] gate {s} <- {url} ({} bytes)", b.len());
                        let _ = std::fs::write("/tmp/probe_body.bin", &b);
                        if s != 200 {
                            eprintln!("[probe] gate body: {}", b.chars().take(300).collect::<String>());
                        }
                    }
                    Err(e) => eprintln!("[probe] gate error: {e}"),
                }
            }
            return;
        }
        match self.raw(url, tokens).await {
            Ok((s, body)) => {
                eprintln!("[probe] direct {s} <- {url} ({} bytes)", body.len());
                let _ = std::fs::write("/tmp/probe_body.bin", &body);
                if s != 200 {
                    eprintln!("[probe] body: {}", body.chars().take(200).collect::<String>());
                }
            }
            Err(e) => eprintln!("[probe] direct error: {e:?}"),
        }
        if let Some(gate) = &self.gate {
            let bearer = tokens
                .id_token
                .clone()
                .unwrap_or_else(|| tokens.access_token.clone());
            match gate.fetch("GET", url, &bearer, None).await {
                Ok((s, body)) => {
                    eprintln!("[probe] gate {s} <- {url}");
                    if s != 200 {
                        eprintln!("[probe] gate body: {}", body.chars().take(200).collect::<String>());
                    }
                }
                Err(e) => eprintln!("[probe] gate error: {e}"),
            }
        }
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
    /// heavier analytics call is skipped and previous stats are kept by the
    /// caller.
    pub async fn fetch_snapshot(
        &self,
        tokens: &TokenSet,
        with_stats: bool,
    ) -> Result<UsageSnapshot, QuotaError> {
        let mut snap = UsageSnapshot {
            fetched_at: Utc::now().timestamp(),
            account_id: tokens.account_id.clone(),
            ..Default::default()
        };

        // The single source of truth for Codex quota: plan, both rate-limit
        // windows, credits, spend control, and reset credits.
        let v = self.get_json(&format!("{BASE}/codex/usage"), tokens).await?;
        snap.plan = dig_string(&v, "plan_type");
        if snap.account_id.is_none() {
            snap.account_id = dig_string(&v, "account_id");
        }

        let rl = v.get("rate_limit");
        for key in ["primary_window", "secondary_window"] {
            let Some(w) = rl.and_then(|r| r.get(key)) else { continue };
            let Some(reset_at) = w.get("reset_at").and_then(Value::as_i64) else { continue };
            let used = w.get("used_percent").and_then(Value::as_f64).unwrap_or(0.0);
            let window_seconds = w.get("limit_window_seconds").and_then(Value::as_i64).unwrap_or(0);
            let is_session = window_seconds > 0 && window_seconds <= 21_600; // <= 6h
            let bucket = RateBucket {
                name: if is_session { "session".into() } else { "weekly".into() },
                window: if is_session { "5h".into() } else { "weekly".into() },
                reset_at,
                maximum_tokens: 100.0,
                remaining_tokens: (100.0 - used).max(0.0),
                used_tokens: used,
                unit: "pct".into(),
            };
            if is_session {
                if snap.session.is_none() {
                    snap.session = Some(bucket);
                }
            } else if snap.weekly.is_none() {
                snap.weekly = Some(bucket);
            }
        }

        if let Some(c) = v.get("credits") {
            snap.system_hard_limit_usd = c
                .get("balance")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<f64>().ok());
        }

        // Reset credits: count from the usage payload, entries from the
        // dedicated endpoint (may be empty).
        let available = v
            .get("rate_limit_reset_credits")
            .and_then(|c| c.get("available_count").and_then(Value::as_i64))
            .unwrap_or(0);
        let mut resets = Vec::new();
        if let Ok(rc) = self
            .get_json(&format!("{BASE}/wham/rate-limit-reset-credits"), tokens)
            .await
        {
            if let Some(entries) = rc.get("credits").and_then(Value::as_array) {
                for e in entries {
                    if let Some(exp) = dig_i64(e, "expires_at").or_else(|| dig_i64(e, "expiration")) {
                        resets.push(ResetCredit {
                            id: dig_string(e, "id").unwrap_or_default(),
                            expires_at: exp,
                        });
                    }
                }
            }
        }
        snap.reset_credits = Some(ResetCredits {
            available_count: available,
            resets,
        });

        // Daily usage history (skipped on routine refreshes).
        if with_stats {
            let start = (Utc::now() - Duration::days(30)).format("%Y-%m-%d");
            let end = Utc::now().format("%Y-%m-%d");
            let url = format!(
                "{BASE}/wham/analytics/daily-plugin-usage-metrics?start_date={start}&end_date={end}&group_by=day"
            );
            if let Ok(daily) = self.get_json(&url, tokens).await {
                snap.stats = compute_stats(&daily);
            }
        }

        Ok(snap)
    }
}

// ---------- daily usage stats ----------

fn compute_stats(daily: &Value) -> Option<UsageStats> {
    let arr = daily
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| daily.get("daily_usage").and_then(Value::as_array))?;
    let mut days: Vec<DayPoint> = arr
        .iter()
        .filter_map(|d| {
            let date = d
                .get("date")
                .or_else(|| d.get("day"))
                .or_else(|| d.get("usage_date"))
                .and_then(Value::as_str)?;
            let tokens = sum_tokens(d);
            Some(DayPoint {
                date: date.to_string(),
                tokens,
            })
        })
        .collect();
    days.sort_by(|a, b| a.date.cmp(&b.date));
    if days.is_empty() {
        return None;
    }

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
    for d in arr {
        for key in ["top_integrations", "integrations", "breakdown", "plugins", "agents"] {
            if let Some(list) = d.get(key).and_then(Value::as_array) {
                for item in list {
                    let name = item
                        .get("name")
                        .or_else(|| item.get("title"))
                        .or_else(|| item.get("integration"))
                        .or_else(|| item.get("plugin"))
                        .or_else(|| item.get("agent"))
                        .and_then(Value::as_str);
                    let val = item
                        .get("tokens")
                        .or_else(|| item.get("value"))
                        .or_else(|| item.get("usage_seconds"))
                        .or_else(|| item.get("count"))
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
