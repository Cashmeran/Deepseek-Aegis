//! IM bridge — Feishu adapter for remote Agent control via mobile messaging.
//! Architecture: Feishu WebSocket → Agent → Feishu REST API.

pub mod feishu;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// A message received from any IM platform, normalized for Agent consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImMessage {
    /// Platform-specific chat/session identifier (for replying).
    pub chat_id: String,
    /// The message text content.
    pub text: String,
    /// Sender display name.
    pub sender: String,
    /// Platform name: "feishu", "telegram", etc.
    pub platform: String,
}

/// Unified IM adapter trait. Each platform implements this.
#[async_trait::async_trait]
pub trait ImAdapter: Send + Sync {
    /// Start listening for incoming messages. Blocks until disconnected.
    /// Each received message is sent as a structured `ImMessage` to `msg_tx`.
    async fn run(
        &self,
        msg_tx: mpsc::UnboundedSender<ImMessage>,
    ) -> Result<(), String>;

    /// Send a text reply back to the IM platform.
    async fn send_reply(&self, chat_id: &str, text: &str) -> Result<(), String>;

    /// Return the platform identifier.
    fn platform(&self) -> &str;
}

/// Forward all stream events from the Agent back to the IM platform.
/// Placeholder — not yet wired. Will be used for streaming output to IM.
#[allow(dead_code)]
pub fn forward_stream_to_im(
    _adapter: std::sync::Arc<dyn ImAdapter>,
    _chat_id: String,
) -> std::sync::Arc<dyn Fn(String) + Send + Sync> {
    std::sync::Arc::new(move |_text: String| {
        // Placeholder for streaming support
    })
}
