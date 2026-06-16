use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
  DeepSeek,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
  Idle,
  Running,
  Completed,
  Error,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
  pub id: String,
  pub title: String,
  pub status: SessionStatus,
  pub cwd: Option<String>,
  pub provider: Option<ProviderKind>,
  pub model: Option<String>,
  pub created_at: i64,
  pub updated_at: i64,
}

// ── Server → Frontend events ──────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum ServerEvent {
  #[serde(rename = "session.list")]
  SessionList { sessions: Vec<SessionInfo> },
  #[serde(rename = "session.history")]
  SessionHistory { #[serde(rename = "sessionId")] session_id: String, status: SessionStatus, messages: Vec<Value> },
  #[serde(rename = "session.status")]
  SessionStatusEvent { #[serde(rename = "sessionId")] session_id: String, status: SessionStatus, #[serde(skip_serializing_if = "Option::is_none")] title: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] cwd: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] error: Option<String> },
  #[serde(rename = "session.deleted")]
  SessionDeleted { #[serde(rename = "sessionId")] session_id: String },
  #[serde(rename = "session.cleared")]
  SessionCleared { #[serde(rename = "sessionId")] session_id: String },

  // Stream events — AgentLoop output
  #[serde(rename = "stream.delta")]
  StreamDelta { #[serde(rename = "sessionId")] session_id: String, text: String },
  #[serde(rename = "stream.thinking")]
  StreamThinking { #[serde(rename = "sessionId")] session_id: String, text: String },
  #[serde(rename = "stream.tool_start")]
  StreamToolStart { #[serde(rename = "sessionId")] session_id: String, id: String, name: String, input: Value },
  #[serde(rename = "stream.tool_result")]
  StreamToolResult { #[serde(rename = "sessionId")] session_id: String, id: String, name: String, is_error: bool, output: String, elapsed_ms: u64 },
  #[serde(rename = "stream.tool_progress")]
  StreamToolProgress { #[serde(rename = "sessionId")] session_id: String, line: String },
  #[serde(rename = "stream.user_prompt")]
  StreamUserPrompt { #[serde(rename = "sessionId")] session_id: String, prompt: String },
  #[serde(rename = "stream.done")]
  StreamDone { #[serde(rename = "sessionId")] session_id: String, input_tokens: u64, output_tokens: u64, cache_read_tokens: u64, cost: f64 },

  #[serde(rename = "ask_user")]
  AskUser { #[serde(rename = "sessionId")] session_id: String, question: String, header: String, options: Vec<Value> },
  #[serde(rename = "runner.error")]
  RunnerError { #[serde(rename = "sessionId")] session_id: Option<String>, message: String },
}

// ── Frontend → Server events ──────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum ClientEvent {
  #[serde(rename = "session.list")]
  SessionList,
  #[serde(rename = "session.history")]
  SessionHistory { #[serde(rename = "sessionId")] session_id: String },
  #[serde(rename = "session.start")]
  SessionStart { title: String, prompt: String, #[serde(skip_serializing_if = "Option::is_none")] cwd: Option<String>, provider: ProviderKind, #[serde(rename = "apiKey")] api_key: String, model: String, #[serde(rename = "baseUrl", skip_serializing_if = "Option::is_none")] base_url: Option<String>, #[serde(rename = "executionMode", skip_serializing_if = "Option::is_none")] execution_mode: Option<String> },
  #[serde(rename = "session.continue")]
  SessionContinue { #[serde(rename = "sessionId")] session_id: String, prompt: String, #[serde(default, skip_serializing_if = "Option::is_none")] messages: Option<Vec<Value>>, #[serde(default, skip_serializing_if = "Option::is_none")] cwd: Option<String> },
  #[serde(rename = "session.stop")]
  SessionStop { #[serde(rename = "sessionId")] session_id: String },
  #[serde(rename = "session.delete")]
  SessionDelete { #[serde(rename = "sessionId")] session_id: String },
  #[serde(rename = "session.compact")]
  SessionCompact { #[serde(rename = "sessionId")] session_id: String },
  #[serde(rename = "session.goal")]
  SessionGoal { #[serde(rename = "sessionId")] session_id: String, objective: String, criteria: Option<String> },
  #[serde(rename = "session.clear")]
  SessionClear { #[serde(rename = "sessionId")] session_id: String },
  #[serde(rename = "ask_user.response")]
  AskUserResponse { #[serde(rename = "sessionId")] session_id: String, answer: String },
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn server_event_serializes() {
    let event = ServerEvent::SessionList { sessions: vec![] };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"session.list\""));
  }
}
