use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::sync::oneshot;

use crate::events::{ProviderKind, SessionInfo, SessionStatus};

#[derive(Clone)]
pub struct ProviderSettings {
  pub provider: ProviderKind,
  pub api_key: String,
  pub model: String,
  pub base_url: Option<String>,
}

#[derive(Default)]
pub struct SessionState {
  sessions: Mutex<HashMap<String, SessionInfo>>,
  messages: Mutex<HashMap<String, Vec<Value>>>,
  providers: Mutex<HashMap<String, ProviderSettings>>,
  modes: Mutex<HashMap<String, String>>,
  cwds: Mutex<HashMap<String, String>>,
  agents: Mutex<HashMap<String, aegis_core::agent::AgentLoop<aegis_core::llm::deepseek::DeepSeekClient>>>,
  pending_permissions: Mutex<HashMap<String, oneshot::Sender<Value>>>,
}

impl SessionState {
  pub fn new() -> Self { Self::default() }

  pub fn list_sessions(&self) -> Vec<SessionInfo> {
    let mut list: Vec<SessionInfo> = self.sessions.lock().expect("session lock").values().cloned().collect();
    list.sort_by_key(|s| s.updated_at);
    list
  }

  pub fn get_session(&self, id: &str) -> Option<SessionInfo> {
    self.sessions.lock().expect("session lock").get(id).cloned()
  }

  pub fn get_messages(&self, id: &str) -> Vec<Value> {
    self.messages.lock().expect("message lock").get(id).cloned().unwrap_or_default()
  }

  pub fn create_session(&self, title: String, cwd: Option<String>) -> SessionInfo {
    let now = now_ms();
    let id = format!("session-{}", now);
    let session = SessionInfo {
      id: id.clone(), title, status: SessionStatus::Running, cwd,
      provider: Some(ProviderKind::DeepSeek),
      model: Some("deepseek-v4-pro".into()),
      created_at: now, updated_at: now,
    };
    self.sessions.lock().expect("session lock").insert(id.clone(), session.clone());
    self.messages.lock().expect("message lock").entry(id.clone()).or_default();
    session
  }

  pub fn update_session(&self, id: &str, status: SessionStatus, title: Option<String>, cwd: Option<String>) -> Option<SessionInfo> {
    let mut sessions = self.sessions.lock().expect("session lock");
    let session = sessions.get_mut(id)?;
    session.status = status;
    if let Some(t) = title { session.title = t; }
    if let Some(c) = cwd { session.cwd = Some(c); }
    session.updated_at = now_ms();
    Some(session.clone())
  }

  pub fn add_message(&self, id: &str, message: Value) {
    let mut messages = self.messages.lock().expect("message lock");
    messages.entry(id.to_string()).or_default().push(message);
  }

  pub fn store_provider(&self, id: &str, settings: ProviderSettings) {
    self.providers.lock().expect("provider lock").insert(id.to_string(), settings);
  }

  pub fn get_provider(&self, id: &str) -> Option<ProviderSettings> {
    self.providers.lock().expect("provider lock").get(id).cloned()
  }

  pub fn store_mode(&self, id: &str, mode: &str) {
    self.modes.lock().expect("mode lock").insert(id.to_string(), mode.to_string());
  }

  pub fn get_mode(&self, id: &str) -> Option<String> {
    self.modes.lock().expect("mode lock").get(id).cloned()
  }

  pub fn store_cwd(&self, id: &str, cwd: &str) {
    self.cwds.lock().expect("cwd lock").insert(id.to_string(), cwd.to_string());
  }

  pub fn get_cwd(&self, id: &str) -> Option<String> {
    self.cwds.lock().expect("cwd lock").get(id).cloned()
  }

  pub fn restore_session(&self, info: SessionInfo) {
    self.sessions.lock().expect("session lock").insert(info.id.clone(), info);
  }

  pub fn remove_session(&self, id: &str) {
    self.sessions.lock().expect("session lock").remove(id);
    self.messages.lock().expect("message lock").remove(id);
    self.providers.lock().expect("provider lock").remove(id);
    self.modes.lock().expect("mode lock").remove(id);
    self.cwds.lock().expect("cwd lock").remove(id);
  }

  /// Set cancellation flag — run_agent_turn checks this between stream events
  pub fn cancel_session(&self, id: &str) {
    if let Ok(mut sessions) = self.sessions.lock() {
      if let Some(s) = sessions.get_mut(id) {
        s.status = SessionStatus::Completed;
      }
    }
  }

  /// Store a persistent AgentLoop for this session
  pub fn store_agent(&self, id: &str, agent: aegis_core::agent::AgentLoop<aegis_core::llm::deepseek::DeepSeekClient>) {
    self.agents.lock().expect("agent lock").insert(id.to_string(), agent);
  }

  /// Take the AgentLoop out (replaces with empty entry)
  pub fn take_agent(&self, id: &str) -> Option<aegis_core::agent::AgentLoop<aegis_core::llm::deepseek::DeepSeekClient>> {
    self.agents.lock().expect("agent lock").remove(id)
  }

  /// Put agent back after turn
  pub fn put_agent(&self, id: &str, agent: aegis_core::agent::AgentLoop<aegis_core::llm::deepseek::DeepSeekClient>) {
    self.agents.lock().expect("agent lock").insert(id.to_string(), agent);
  }

  pub fn list_recent_cwds(&self, limit: usize) -> Vec<String> {
    let mut list: Vec<SessionInfo> = self.sessions.lock().expect("session lock").values().cloned().collect();
    list.sort_by_key(|s| s.updated_at);
    list.reverse();
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for s in list {
      if let Some(cwd) = s.cwd {
        if seen.insert(cwd.clone()) { result.push(cwd); }
      }
      if result.len() >= limit { break; }
    }
    result
  }
}

fn now_ms() -> i64 {
  SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
