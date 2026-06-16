//! IM bridge — Feishu WebSocket listener that routes messages to/from the Agent.
//!
//! Architecture:
//!   Feishu WS → ImMessage → msg_rx → AgentLoop::run() → send_reply() → Feishu REST API
//!
//! Config stored in ~/.aegis/im.toml

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use aegis_im::feishu::{FeishuAdapter, FeishuConfig};
use aegis_im::{ImAdapter, ImMessage};
use serde::{Deserialize, Serialize};

use tauri::{Emitter, Manager};

use crate::commands::client_event;

/// Global bridge + config store.
static BRIDGE: OnceLock<Mutex<Option<Arc<ImBridge>>>> = OnceLock::new();
/// Last connection error surfaced to the UI.
static LAST_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn bridge_lock() -> &'static Mutex<Option<Arc<ImBridge>>> {
    BRIDGE.get_or_init(|| Mutex::new(None))
}
fn error_lock() -> &'static Mutex<Option<String>> {
    LAST_ERROR.get_or_init(|| Mutex::new(None))
}

// ── IM Config persistence ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImConfig {
    pub platform: String,
    pub app_id: String,
    pub app_secret: String,
    #[serde(default)]
    pub enabled: bool,
}

impl Default for ImConfig {
    fn default() -> Self {
        Self { platform: "feishu".into(), app_id: String::new(), app_secret: String::new(), enabled: false }
    }
}

fn im_config_path() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_default().join(".aegis").join("im.toml")
}

pub fn load_im_config() -> ImConfig {
    let path = im_config_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<ImConfig>(&content) {
            return cfg;
        }
    }
    let mut cfg = ImConfig::default();
    if let Ok(id) = std::env::var("FEISHU_APP_ID") {
        if !id.is_empty() { cfg.app_id = id; cfg.enabled = true; }
    }
    if let Ok(secret) = std::env::var("FEISHU_APP_SECRET") {
        if !secret.is_empty() { cfg.app_secret = secret; cfg.enabled = true; }
    }
    cfg
}

fn write_im_config(cfg: &ImConfig) -> Result<(), String> {
    let path = im_config_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let toml_str = toml::to_string_pretty(cfg).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(&path, toml_str).map_err(|e| format!("写入失败: {e}"))
}

// ── Bridge status ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImStatus {
    pub platform: String,
    pub configured: bool,
    pub enabled: bool,
    pub connected: bool,
    pub app_id: String,
    pub app_secret: String,
    /// Current project directory (from last notify_im_project call).
    pub project_dir: Option<String>,
    pub last_error: Option<String>,
}

pub fn get_im_status() -> ImStatus {
    let cfg = load_im_config();
    let guard = bridge_lock().lock().unwrap();
    let is_ws_connected = guard.as_ref().map(|b| b.ws_connected.load(Ordering::SeqCst)).unwrap_or(false);
    let err = error_lock().lock().unwrap().clone();
    let project = guard.as_ref().and_then(|b| b.cwd.lock().ok()).map(|c| c.clone());
    ImStatus {
        platform: cfg.platform,
        configured: !cfg.app_id.is_empty() && !cfg.app_secret.is_empty(),
        enabled: cfg.enabled && !cfg.app_id.is_empty(),
        connected: is_ws_connected,
        app_id: cfg.app_id,
        app_secret: cfg.app_secret,
        project_dir: project,
        last_error: err,
    }
}

fn set_last_error(msg: Option<String>) {
    *error_lock().lock().unwrap() = msg;
}

// ── Bridge struct ──────────────────────────────────────────────────

pub struct ImBridge {
    pub adapter: Arc<FeishuAdapter>,
    app_handle: tauri::AppHandle,
    api_key: String,
    model: String,
    cwd: Mutex<String>,
    shutdown: Arc<AtomicBool>,
    ws_connected: Arc<AtomicBool>,
}

impl ImBridge {
    pub fn start_from_config(app_handle: tauri::AppHandle) -> Option<Arc<Self>> {
        let cfg = load_im_config();
        if !cfg.enabled || cfg.app_id.is_empty() || cfg.app_secret.is_empty() {
            log::info!("IM bridge disabled or not configured");
            return None;
        }
        let cwd = std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let bridge = Self::launch(&cfg.app_id, &cfg.app_secret, &cfg.platform, &cwd, app_handle);
        let mut guard = bridge_lock().lock().unwrap();
        *guard = Some(Arc::clone(&bridge));
        Some(bridge)
    }

    pub fn reload(app_id: &str, app_secret: &str, platform: &str) -> Option<Arc<Self>> {
        let (cwd, app_handle) = {
            let guard = bridge_lock().lock().unwrap();
            let cwd = guard.as_ref()
                .and_then(|b| b.cwd.lock().ok())
                .map(|c| c.clone())
                .unwrap_or_default();
            let ah = guard.as_ref().map(|b| b.app_handle.clone());
            (cwd, ah)
        };
        // Shutdown old
        if let Some(old) = bridge_lock().lock().unwrap().take() {
            old.shutdown.store(true, Ordering::SeqCst);
        }
        let app_handle = app_handle.unwrap_or_else(|| {
            panic!("IM bridge reload called without existing bridge");
        });
        let bridge = Self::launch(app_id, app_secret, platform, &cwd, app_handle);
        let mut guard = bridge_lock().lock().unwrap();
        *guard = Some(Arc::clone(&bridge));
        Some(bridge)
    }

    pub fn stop() {
        if let Some(old) = bridge_lock().lock().unwrap().take() {
            old.shutdown.store(true, Ordering::SeqCst);
        }
        set_last_error(None);
    }

    /// Update the project working directory + active session (called when user switches project).
    pub fn update_cwd_and_session(new_cwd: &str, _session_id: &str) {
        if let Some(bridge) = bridge_lock().lock().unwrap().as_ref() {
            if let Ok(mut c) = bridge.cwd.lock() {
                *c = new_cwd.to_string();
            }
        }
    }

    /// Get the current project directory (for frontend display).
    pub fn current_cwd() -> Option<String> {
        bridge_lock().lock().ok()?
            .as_ref()?
            .cwd.lock().ok()?
            .clone().into()
    }

    fn launch(app_id: &str, app_secret: &str, brand: &str, cwd: &str, app_handle: tauri::AppHandle) -> Arc<Self> {
        let (api_key, model) = client_event::read_api_key_internal();
        let brand = brand.to_string();
        let cwd = cwd.to_string();

        let config = FeishuConfig {
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            brand: brand.clone(),
        };

        let adapter = Arc::new(FeishuAdapter::new(config));
        let shutdown = Arc::new(AtomicBool::new(false));
        let ws_connected = Arc::new(AtomicBool::new(false));
        let bridge = Arc::new(ImBridge {
            adapter: adapter.clone(),
            app_handle: app_handle.clone(),
            api_key,
            model,
            cwd: Mutex::new(cwd),
            shutdown: shutdown.clone(),
            ws_connected: ws_connected.clone(),
        });

        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<ImMessage>();

        // Task 1: WebSocket listener
        let listener_adapter = Arc::clone(&adapter);
        let listener_shutdown = Arc::clone(&shutdown);
        let listener_connected = Arc::clone(&ws_connected);
        let ws_brand = brand.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                if listener_shutdown.load(Ordering::SeqCst) { break; }
                log::info!("IM bridge connecting ({ws_brand})...");
                set_last_error(None);
                listener_connected.store(false, Ordering::SeqCst);
                match listener_adapter.run(msg_tx.clone()).await {
                    Ok(()) => {}
                    Err(e) => {
                        if listener_shutdown.load(Ordering::SeqCst) { break; }
                        let msg = e.to_string();
                        // Clean disconnect — expected on shutdown
                        if msg.contains("ended") { set_last_error(None); }
                        // 404 = app not published or long connection not enabled
                        else if msg.contains("404") {
                            set_last_error(Some("飞书应用未启用长连接。请到开放平台 → 事件订阅 → 选择「使用长连接接收事件」→ 订阅 im.message.receive_v1 → 发布应用并等待审核通过".into()));
                        }
                        // Auth errors
                        else if msg.contains("401") || msg.contains("403") {
                            set_last_error(Some(format!("飞书认证失败，请检查 App ID / Secret 是否正确: {msg}")));
                        }
                        // Other
                        else {
                            set_last_error(Some(format!("连接失败: {msg}")));
                        }
                        log::error!("IM WS error: {e}, reconnecting in 5s");
                        listener_connected.store(false, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
            listener_connected.store(false, Ordering::SeqCst);
            log::info!("IM bridge listener stopped");
        });

        // Task 2: Message processor — emits Tauri events to frontend
        let bridge_processor = Arc::clone(&bridge);
        let proc_shutdown = Arc::clone(&shutdown);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    msg = msg_rx.recv() => {
                        match msg {
                            Some(im_msg) => {
                                // Sentinel: WS connected
                                if im_msg.sender == "__connected__" {
                                    bridge_processor.ws_connected.store(true, Ordering::SeqCst);
                                    set_last_error(None);
                                    log::info!("IM WS connected successfully");
                                    continue;
                                }

                                // Emit to frontend — the frontend will send session.continue
                                let payload = serde_json::json!({
                                    "chatId": im_msg.chat_id,
                                    "text": im_msg.text,
                                    "sender": im_msg.sender,
                                    "platform": im_msg.platform,
                                });
                                if let Err(e) = bridge_processor.app_handle.emit("im-message", &payload) {
                                    log::error!("Failed to emit im-message: {e}");
                                }
                            }
                            None => break,
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                        if proc_shutdown.load(Ordering::SeqCst) && msg_rx.is_empty() {
                            break;
                        }
                    }
                }
            }
        });

        log::info!("IM bridge launched (brand: {brand})");
        bridge
    }

}

/// Send a reply back to an IM chat. Called by frontend after agent finishes.
#[tauri::command]
pub async fn send_im_reply(chat_id: String, text: String) -> Result<(), String> {
    let adapter = {
        bridge_lock().lock().unwrap()
            .as_ref()
            .map(|b| b.adapter.clone())
    };
    match adapter {
        Some(a) => a.send_reply(&chat_id, &text).await,
        None => Err("IM bridge not active".into()),
    }
}

// ── Tauri Commands ──────────────────────────────────────────────────

#[tauri::command]
pub fn get_im_config() -> Result<ImStatus, String> {
    Ok(get_im_status())
}

#[tauri::command]
pub async fn test_im_connection(app_id: String, app_secret: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
        .json(&serde_json::json!({
            "app_id": app_id,
            "app_secret": app_secret,
        }))
        .send()
        .await
        .map_err(|e| format!("网络请求失败: {e}"))?;

    #[derive(Deserialize)]
    struct TokenResp {
        code: i32,
        #[serde(default)] msg: String,
        #[serde(default)] tenant_access_token: String,
    }

    let body: TokenResp = resp.json().await.map_err(|e| format!("解析响应失败: {e}"))?;
    if body.code != 0 {
        return Err(format!("飞书返回错误: code={}, msg={}", body.code, body.msg));
    }
    if body.tenant_access_token.is_empty() {
        return Err("飞书返回空 token".into());
    }
    Ok(format!("飞书凭证有效 (token: {}…)", &body.tenant_access_token[..8.min(body.tenant_access_token.len())]))
}

#[tauri::command]
pub async fn save_im_config(app: tauri::AppHandle, platform: String, app_id: String, app_secret: String, enabled: Option<bool>) -> Result<ImStatus, String> {
    let enabled = enabled.unwrap_or(true);
    let cfg = ImConfig {
        platform: platform.clone(),
        app_id: app_id.clone(),
        app_secret: app_secret.clone(),
        enabled: enabled && !app_id.is_empty() && !app_secret.is_empty(),
    };
    write_im_config(&cfg)?;

    if cfg.enabled {
        let app_id2 = app_id.clone();
        let app_secret2 = app_secret.clone();
        let platform2 = platform.clone();
        let ah = app.clone();
        // If bridge already exists, reload. Otherwise start fresh.
        let has_bridge = bridge_lock().lock().unwrap().is_some();
        tauri::async_runtime::spawn(async move {
            if has_bridge {
                ImBridge::reload(&app_id2, &app_secret2, &platform2);
            } else {
                let cwd = std::env::current_dir().ok()
                    .map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                let bridge = ImBridge::launch(&app_id2, &app_secret2, &platform2, &cwd, ah);
                *bridge_lock().lock().unwrap() = Some(bridge);
            }
        });
    } else {
        ImBridge::stop();
    }

    Ok(get_im_status())
}

/// Notify the IM bridge of the active project + session.
/// Also updates the backend session's cwd so the agent works in the right directory.
#[tauri::command]
pub fn notify_im_project(app: tauri::AppHandle, cwd: String, session_id: String) {
    ImBridge::update_cwd_and_session(&cwd, &session_id);
    // Update backend session state's cwd too
    let state = app.state::<crate::state::SessionState>();
    state.store_cwd(&session_id, &cwd);
    log::info!("IM bridge project updated: cwd={cwd}, session={session_id}");
}
