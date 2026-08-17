//! Cloudflare gate.
//!
//! `chatgpt.com/backend-api` sits behind Cloudflare's managed challenge: a
//! plain HTTP client gets a 403 challenge page, and only a real browser
//! engine (which solves the challenge and keeps `cf_clearance`) is let
//! through. CodexDesk therefore keeps a tiny off-screen webview parked on
//! chatgpt.com. The injected `window.__deskBridge` runs same-origin fetches
//! for us and reports results back over a tauri event.
//!
//! No cookies are scraped from the user's browser, nothing is executed that
//! we didn't write, and the page only ever talks to chatgpt.com.

use rand::RngCore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Listener, Manager, WebviewWindow};
use tokio::sync::oneshot;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateReply {
    pub id: String,
    pub status: u16,
    pub body: String,
}

#[derive(Clone)]
pub struct Gate {
    window: Option<WebviewWindow<tauri::Wry>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<GateReply>>>>,
}

/// Injected into every load of the gate page. Runs same-origin fetches on
/// chatgpt.com and reports results through the `gate://reply` event.
const GATE_SCRIPT: &str = "window.__deskBridge=function(id,method,url,bearer,body){var opts={method:method||'GET',headers:{Authorization:'Bearer '+bearer,Accept:'application/json, text/plain, */*','Accept-Language':'en-US,en;q=0.9',Referer:'https://chatgpt.com/'},credentials:'include'};if(body){opts.headers['Content-Type']='application/json';opts.body=body;}fetch(url,opts).then(function(r){return r.text().then(function(t){window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'gate://reply',payload:{id:id,status:r.status,body:t}})})}).catch(function(e){window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'gate://reply',payload:{id:id,status:0,body:String(e)}})})};";

impl Gate {
    /// Create the off-screen gate window (called once during setup).
    pub fn ensure_window(app: &mut tauri::App<tauri::Wry>) -> tauri::Result<()> {
        use tauri::WebviewUrl;
        let url: tauri::Url = "https://chatgpt.com/".parse().expect("static url");
        let window = tauri::WebviewWindowBuilder::new(app, "cf-gate", WebviewUrl::External(url))
            .inner_size(320.0, 320.0)
            .position(-32000.0, -32000.0)
            .decorations(false)
            .skip_taskbar(true)
            .resizable(false)
            .visible(false)
            .initialization_script(GATE_SCRIPT)
            .build()?;
        let _ = window.show();
        Ok(())
    }

    pub fn new(app: &AppHandle<tauri::Wry>) -> Self {
        Self {
            window: app.get_webview_window("cf-gate"),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Called from the `gate://reply` listener.
    pub fn on_reply(&self, reply: GateReply) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&reply.id) {
            let _ = tx.send(reply);
        }
    }

    /// Fetch a URL through the gate webview. Returns (status, body).
    pub async fn fetch(
        &self,
        method: &str,
        url: &str,
        bearer: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), String> {
        let win = self.window.as_ref().ok_or("gate window is not available")?;

        let id = {
            let mut b = [0u8; 12];
            rand::rng().fill_bytes(&mut b);
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        };
        let (tx, mut rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);

        // Escape the JS string arguments.
        let esc = |s: &str| s.replace('\\', "\\\\").replace('\'', "\\'");
        let url_js = esc(url);
        let bearer_js = esc(bearer);
        let body_js = body.map(|b| format!("'{}'", esc(b))).unwrap_or("null".into());
        let js = format!(
            "window.__deskBridge && window.__deskBridge('{id}', '{method}', '{url_js}', '{bearer_js}', {body_js})"
        );

        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(45);
        let mut reloaded = false;

        loop {
            let _ = win.eval(&js);
            match tokio::time::timeout(std::time::Duration::from_secs(8), &mut rx).await {
                Ok(Ok(reply)) => {
                    self.pending.lock().unwrap().remove(&id);
                    #[cfg(debug_assertions)]
                    eprintln!("[gate] {} -> {}", reply.status, url);
                    return Ok((reply.status, reply.body));
                }
                Ok(Err(_)) => break, // channel dropped
                Err(_) => {
                    if tokio::time::Instant::now() >= deadline {
                        break;
                    }
                    if !reloaded {
                        // The challenge page may still be mid-flight; give it
                        // one reload and keep polling.
                        let _ = win.eval("location.reload()");
                        reloaded = true;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                }
            }
        }

        self.pending.lock().unwrap().remove(&id);
        Err(
            "Cloudflare challenge not cleared — open chatgpt.com once in a browser, then retry"
                .into(),
        )
    }
}

pub fn listen(app: &AppHandle<tauri::Wry>) {
    let gate = app.state::<Gate>().inner().clone();
    let _ = app.listen("gate://reply", move |event| {
        if let Ok(reply) = serde_json::from_str::<GateReply>(event.payload()) {
            gate.on_reply(reply);
        }
    });
}