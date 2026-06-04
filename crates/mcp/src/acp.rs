//! ACP (Agent Communication Protocol) — HTTP/SSE agent interface.
//! Spec: https://github.com/google/acp
//!
//! Endpoints:
//!   GET  /agents              → list available agents
//!   POST /agents/{id}/chat     → send message, get JSON response
//!   GET  /agents/{id}/stream   → SSE stream (Server-Sent Events)
//!   POST /agents/{id}/cancel   → cancel running task

use axum::{
    extract::{Path, State},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot};

// ═══════════════ ACP Types ═══════════════

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub tokens_used: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AcpStreamEvent {
    #[serde(rename = "text")]
    Text { content: String },
    #[serde(rename = "tool_use")]
    ToolUse { name: String, input: serde_json::Value },
    #[serde(rename = "tool_result")]
    ToolResult { name: String, output: String },
    #[serde(rename = "done")]
    Done { model: String, tokens_used: u64 },
    #[serde(rename = "error")]
    Error { message: String },
}

// ═══════════════ ACP Server State ═══════════════

/// Shared state between ACP routes and the agent loop.
pub struct AcpState {
    /// Sends user messages to the agent.
    pub agent_tx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedSender<String>>,
    /// Broadcasts stream events from the agent to all SSE clients.
    pub stream_tx: broadcast::Sender<AcpStreamEvent>,
    /// Agent metadata.
    pub agents: Vec<AgentInfo>,
    /// Cancellation signal.
    pub cancel_tx: tokio::sync::Mutex<Option<oneshot::Sender<()>>>,
}

impl AcpState {
    pub fn new(
        agent_tx: tokio::sync::mpsc::UnboundedSender<String>,
        stream_tx: broadcast::Sender<AcpStreamEvent>,
    ) -> Self {
        Self {
            agent_tx: tokio::sync::Mutex::new(agent_tx),
            stream_tx,
            agents: vec![
                AgentInfo {
                    name: "aegis".into(),
                    description: "aegis AI coding agent — powered by DeepSeek V4".into(),
                    model: "deepseek-v4-pro".into(),
                },
            ],
            cancel_tx: tokio::sync::Mutex::new(None),
        }
    }
}

// ═══════════════ Routes ═══════════════

pub fn acp_router(state: Arc<AcpState>) -> Router {
    Router::new()
        .route("/agents", get(list_agents))
        .route("/agents/{id}/chat", post(agent_chat))
        .route("/agents/{id}/stream", get(agent_stream))
        .route("/agents/{id}/cancel", post(agent_cancel))
        .with_state(state)
}

/// GET /agents — list all available agents.
async fn list_agents(State(state): State<Arc<AcpState>>) -> Json<Vec<AgentInfo>> {
    Json(state.agents.clone())
}

/// POST /agents/{id}/chat — send a message, get the full response.
async fn agent_chat(
    State(state): State<Arc<AcpState>>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (axum::http::StatusCode, String)> {
    if id != "aegis" {
        return Err((axum::http::StatusCode::NOT_FOUND, format!("Agent '{}' not found", id)));
    }

    // Send message to agent
    state.agent_tx.lock().await
        .send(req.message.clone())
        .map_err(|e| (axum::http::StatusCode::SERVICE_UNAVAILABLE, format!("Agent unavailable: {}", e)))?;

    // Subscribe to stream and collect full response
    let mut rx = state.stream_tx.subscribe();
    let mut content = String::new();
    let mut model = String::from("deepseek-v4-pro");
    let mut tokens = 0u64;

    loop {
        match rx.recv().await {
            Ok(AcpStreamEvent::Text { content: text }) => {
                content.push_str(&text);
            }
            Ok(AcpStreamEvent::Done { model: m, tokens_used }) => {
                model = m;
                tokens = tokens_used;
                break;
            }
            Ok(AcpStreamEvent::Error { message }) => {
                return Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR, message));
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => break,
            _ => {} // skip tool_use/tool_result for non-streaming
        }
    }

    if content.is_empty() {
        return Err((axum::http::StatusCode::REQUEST_TIMEOUT, "No response from agent".into()));
    }

    Ok(Json(ChatResponse { content, model, tokens_used: tokens }))
}

/// GET /agents/{id}/stream — SSE stream of agent response events.
async fn agent_stream(
    State(state): State<Arc<AcpState>>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (axum::http::StatusCode, String)> {
    if id != "aegis" {
        return Err((axum::http::StatusCode::NOT_FOUND, format!("Agent '{}' not found", id)));
    }

    let rx = state.stream_tx.subscribe();

    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let (name, data) = match &event {
                        AcpStreamEvent::Text { content } => ("text", content.clone()),
                        AcpStreamEvent::ToolUse { name: n, input } => {
                            ("tool_use", serde_json::to_string(&serde_json::json!({"name": n, "input": input})).unwrap_or_default())
                        }
                        AcpStreamEvent::ToolResult { name: n, output } => {
                            ("tool_result", format!("{}: {}", n, output.chars().take(200).collect::<String>()))
                        }
                        AcpStreamEvent::Done { model, tokens_used } => {
                            ("done", format!("{} | {} tokens", model, tokens_used))
                        }
                        AcpStreamEvent::Error { message } => ("error", message.clone()),
                    };
                    let sse = Event::default().event(name).data(data);
                    return Some((Ok(sse), rx));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    Ok(Sse::new(stream))
}

/// POST /agents/{id}/cancel — cancel the currently running task.
async fn agent_cancel(
    State(state): State<Arc<AcpState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    if id != "aegis" {
        return Err((axum::http::StatusCode::NOT_FOUND, format!("Agent '{}' not found", id)));
    }

    let mut cancel = state.cancel_tx.lock().await;
    if let Some(tx) = cancel.take() {
        let _ = tx.send(());
        Ok(Json(serde_json::json!({"status": "cancelled"})))
    } else {
        Ok(Json(serde_json::json!({"status": "no_task_running"})))
    }
}
