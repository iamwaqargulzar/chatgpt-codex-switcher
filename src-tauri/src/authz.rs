use crate::desktop::open_url;
use crate::error::{AppError, R};
use crate::models::{Account, AuthBundle, AuthKind, TokenSet, WarmupConfig};
use crate::paths::Paths;
use crate::quota::{jwt_claim, QuotaClient};
use crate::vault::Vault;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Public OAuth constants of the official Codex CLI login client.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const SCOPES: &str = "openid profile email offline_access api.connectors.read api.connectors.invoke";

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn authorize_url(challenge: &str, state: &str) -> String {
    format!(
        "https://auth.openai.com/oauth/authorize?response_type=code&client_id={CLIENT_ID}&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&id_token_add_organizations=true&codex_cli_simplified_flow=true&state={state}&originator=codexdesk",
        url::form_urlencoded::byte_serialize(REDIRECT_URI.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(SCOPES.as_bytes()).collect::<String>(),
    )
}

/// Full ChatGPT login: opens the browser, waits for the localhost callback,
/// exchanges the code, and stores the account. Progress is reported through
/// `auth://progress`, the outcome through `auth://done`.
pub async fn start_oauth(app: AppHandle, vault: Arc<Mutex<Vault>>, paths: Paths, http: QuotaClient) {
    let fail = |msg: String| {
        let _ = app.emit(
            "auth://done",
            json!({ "ok": false, "error": msg, "account": null }),
        );
    };

    let _ = app.emit("auth://progress", "opening browser");
    let verifier = random_token(64);
    let state = random_token(32);
    let url = authorize_url(&challenge(&verifier), &state);

    if let Err(e) = open_url(&url) {
        fail(format!("could not open a browser: {e}"));
        return;
    }

    let _ = app.emit("auth://progress", "waiting for approval");
    let code = match listen_for_callback(&state).await {
        Ok(c) => c,
        Err(e) => {
            fail(e);
            return;
        }
    };

    let _ = app.emit("auth://progress", "exchanging code for tokens");
    let tokens = match exchange_code(&http, &code, &verifier).await {
        Ok(t) => t,
        Err(e) => {
            fail(e.to_string());
            return;
        }
    };

    let _ = app.emit("auth://progress", "identifying account");
    let account_id = match http.check_account(&tokens).await {
        Ok(id) => id,
        Err(e) => {
            fail(format!("could not identify the account: {e}"));
            return;
        }
    };
    let email = tokens
        .id_token
        .as_deref()
        .and_then(|t| jwt_claim(t, "email"));

    let mut account = Account {
        id: uuid_like(),
        name: email
            .as_deref()
            .and_then(|e| e.split('@').next())
            .unwrap_or("ChatGPT account")
            .to_string(),
        email,
        kind: AuthKind::Chatgpt,
        profile: None,
        added_at: chrono::Utc::now().timestamp(),
        last_used_at: None,
        warmup: WarmupConfig::default(),
        auth: AuthBundle {
            openai_api_key: None,
            tokens: Some(tokens),
        },
    };
    account.auth.tokens.as_mut().map(|t| t.account_id = account_id);

    {
        let mut vault = vault.lock().unwrap();
        if vault.accounts().is_empty() {
            // First account becomes the active one immediately.
            if let Err(e) = crate::switch::backup_current(&paths).and_then(|_| crate::switch::write_auth_json(&paths, &account)).and_then(|_| vault.set_active(Some(&account.id))) {
                fail(format!("account added but activating it failed: {e}"));
                return;
            }
        }
        let name = account.name.clone();
        let id = account.id.clone();
        let public = account.public(vault.active_id().as_deref() == Some(id.as_str()), None, None);
        if let Err(e) = vault.upsert(account) {
            fail(format!("could not store the account: {e}"));
            return;
        }
        let _ = app.emit("vault://changed", json!({}));
        let _ = app.emit(
            "auth://done",
            json!({ "ok": true, "error": null, "account": { "id": id, "name": name, "public": public } }),
        );
    }
}

/// Exchange the OAuth code for tokens at the official endpoint.
async fn exchange_code(http: &QuotaClient, code: &str, verifier: &str) -> R<TokenSet> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", CLIENT_ID),
        ("code_verifier", verifier),
    ];
    let resp = http
        .token_request(&form)
        .await
        .map_err(|e| AppError::Msg(format!("token exchange failed: {e}")))?;
    let v: serde_json::Value = resp;
    if v.get("error").is_some() {
        return Err(AppError::Msg("OpenAI rejected the login".into()));
    }
    Ok(crate::quota::merge_tokens_response(&v))
}

/// Wait on 127.0.0.1:1455 for the browser redirect carrying `?code=...`.
async fn listen_for_callback(state: &str) -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:1455")
        .await
        .map_err(|_| {
            "port 1455 is busy — close any running `codex login` or CodexDesk login first".to_string()
        })?;

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        let timeout = tokio::time::timeout_at(deadline, listener.accept());
        let (mut stream, _) = timeout
            .await
            .map_err(|_| "login timed out after 5 minutes".to_string())?
            .map_err(|e| e.to_string())?;

        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
        let request = String::from_utf8_lossy(&buf[..n]).to_string();

        let Some(path) = request.lines().next().and_then(|l| l.split_whitespace().nth(1)) else {
            continue;
        };

        if !path.contains("callback") {
            send_html(stream, NOT_CALLBACK_HTML).await;
            continue;
        }

        let query = path.split('?').nth(1).unwrap_or("");
        let params: std::collections::HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        let got_state = params.get("state").cloned().unwrap_or_default();
        if got_state != state {
            send_html(stream, BAD_STATE_HTML).await;
            continue;
        }
        if let Some(err) = params.get("error") {
            let desc = params.get("error_description").cloned().unwrap_or_default();
            send_html(stream, &format!("Login failed: {err} — {desc}")).await;
            return Err(format!("OpenAI denied the login: {err}"));
        }
        match params.get("code").cloned() {
            Some(code) => {
                send_html(stream, SUCCESS_HTML).await;
                return Ok(code);
            }
            None => {
                send_html(stream, BAD_STATE_HTML).await;
            }
        }
    }
}

async fn send_html(mut stream: tokio::net::TcpStream, body: &str) {
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes()).await;
}

fn uuid_like() -> String {
    // 128-bit random id, formatted like a uuid without pulling a uuid crate.
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    let hex = b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

const SUCCESS_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>CodexDesk</title><style>body{font-family:system-ui;background:#0d1117;color:#e6edf3;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}div{text-align:center}h1{font-size:20px}p{color:#8b949e}</style></head><body><div><h1>✓ Signed in to CodexDesk</h1><p>You can close this tab and return to the app.</p></div></body></html>";

const BAD_STATE_HTML: &str = "<!doctype html><html><body style=\"font-family:system-ui;padding:40px\"><h1>Invalid login state</h1><p>Please start the login again from CodexDesk.</p></body></html>";

const NOT_CALLBACK_HTML: &str = "<!doctype html><html><body style=\"font-family:system-ui;padding:40px\"><h1>CodexDesk callback port</h1><p>This port is only used while a login is in progress.</p></body></html>";
