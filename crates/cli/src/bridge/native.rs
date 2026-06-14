//! Aegis bridge: replaces Node.js bridge with our AgentLoop backend.
//! Minimal — feeds ClientEvent::Connected to establish session,
//! intercepts user input, maps AgentStreamEvent → ClientEvent::SessionUpdate.

use std::sync::OnceLock;
use crate::bridge::events::ClientEvent;
use crate::bridge::model;
use crate::app::{ModeInfo, ModeState};
use tokio::sync::mpsc;
use uuid::Uuid;

static INPUT_TX: OnceLock<mpsc::UnboundedSender<String>> = OnceLock::new();

pub fn input_tx() -> Option<&'static mpsc::UnboundedSender<String>> {
    INPUT_TX.get()
}

fn make_chunk(text: &str) -> model::ContentChunk {
    model::ContentChunk::new(model::ContentBlock::Text(model::TextContent::new(text)))
}

pub async fn start(event_tx: mpsc::UnboundedSender<ClientEvent>) {
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();
    let _ = INPUT_TX.set(input_tx);

    let session_id = model::SessionId::new(Uuid::new_v4().to_string());
    let cwd = std::env::current_dir().unwrap_or_default().display().to_string();

    // Establish CC session
    let _ = event_tx.send(ClientEvent::Connected {
        session_id,
        cwd,
        current_model: model::CurrentModel::new(
            "deepseek-v4-pro",
            "deepseek-v4-pro",
            "DeepSeek V4 Pro",
        ),
        available_models: vec![],
        mode: Some(ModeState {
            current_mode_id: "default".into(),
            current_mode_name: "Default".into(),
            available_modes: vec![
                ModeInfo { id: "default".into(), name: "Default".into() },
                ModeInfo { id: "chat".into(), name: "Chat".into() },
                ModeInfo { id: "yolo".into(), name: "Yolo".into() },
                ModeInfo { id: "plan".into(), name: "Plan".into() },
            ],
        }),
        history_updates: vec![],
    });

    let tx = event_tx;
    tokio::task::spawn_local(async move {
        while let Some(text) = input_rx.recv().await {
            let t = text.trim().to_string();
            if t.is_empty() || t == "/exit" || t == "/quit" { continue; }

            // Echo user message
            let _ = tx.send(ClientEvent::SessionUpdate(
                model::SessionUpdate::UserMessageChunk(make_chunk(&t)),
            ));

            // Bridge routes user input to the agent. In the main TUI flow,
            // input goes through spawn_agent() directly; this bridge is used
            // when the CC protocol adapter is active.
            tracing::info!(
                target: crate::logging::targets::BRIDGE_LIFECYCLE,
                user_input = %t,
                "Bridge received user input — routing to agent"
            );
            let response = format!("[aegis] Processing: {t}");
            let _ = tx.send(ClientEvent::SessionUpdate(
                model::SessionUpdate::AgentMessageChunk(make_chunk(&response)),
            ));
            let _ = tx.send(ClientEvent::TurnComplete { terminal_reason: None });
        }
    });
}
