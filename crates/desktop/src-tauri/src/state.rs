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

  pub fn remove_session(&self, id: &str) {
    self.sessions.lock().expect("session lock").remove(id);
    self.messages.lock().expect("message lock").remove(id);
    self.providers.lock().expect("provider lock").remove(id);
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
