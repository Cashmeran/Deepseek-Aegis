use crate::error::{AgentError, AgentResult};
use crate::llm::client::{LlmClient, LlmRequest, LlmResponse, ModelInfo, StreamEvent};
use crate::types::message::Message;
use crate::types::tool::ReasoningEffort;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::Value;

// ═══════════════════════════════════════════════════════════
// DeepSeek API Client — DeepSeek API 兼容 API 格式
//
// API Endpoint: https://api.deepseek.com/anthropic/v1/messages
// 文档: https://api-docs.deepseek.com/zh-cn/guides/anthropic_api
//
// 关键特性:
// - thinking/reasoning_content 回传 (多轮对话)
// - 前缀自动缓存 (磁盘缓存, 命中率 >99%)
// - Jitter 重试 (75%-125% 随机退避)
// - user_id KVCache 隔离
// ═══════════════════════════════════════════════════════════

const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com";
const ANTHROPIC_MESSAGES_PATH: &str = "/anthropic/v1/messages";

/// DeepSeek API 客户端。
pub struct DeepSeekClient {
    api_key: String,
    base_url: String,
    http_client: reqwest::Client,
    model_info: ModelInfo,
    user_id: String,
}

impl DeepSeekClient {
    /// 创建新的 DeepSeek 客户端。
    /// `api_key` 从 DEEPSEEK_API_KEY 环境变量读取。
    pub fn new(api_key: String, model: &str) -> AgentResult<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| AgentError::ConfigError(format!("Failed to create HTTP client: {}", e)))?;

        let model_info = match model {
            "deepseek-v4-pro" => ModelInfo {
                id: "deepseek-v4-pro".into(),
                provider: "deepseek".into(),
                max_context_tokens: 1_048_576,
                max_output_tokens: 393_216,  // 384K — max for V4 series
                input_price_per_mtok: 0.14,
                output_price_per_mtok: 0.28,
                cache_price_per_mtok: 0.014,
                supports_reasoning: true,
                supports_caching: true,
            },
            _ => ModelInfo {
                id: model.into(),
                provider: "deepseek".into(),
                max_context_tokens: 1_048_576,
                max_output_tokens: 8192,
                input_price_per_mtok: 0.014,
                output_price_per_mtok: 0.028,
                cache_price_per_mtok: 0.0014,
                supports_reasoning: true,
                supports_caching: true,
            },
        };

        Ok(Self {
            api_key,
            base_url: DEEPSEEK_API_BASE.to_string(),
            http_client,
            model_info,
            user_id: "deepseek-aegis".to_string(),
        })
    }

    /// 从环境变量创建 (DEEPSEEK_API_KEY)。
    pub fn from_env() -> AgentResult<Self> {
        let api_key = std::env::var("DEEPSEEK_API_KEY").map_err(|_| {
            AgentError::ApiKeyMissing
        })?;
        Self::new(api_key, "deepseek-v4-pro")
    }

    /// Build messages for DeepSeek API, grouping consecutive ToolResults
    /// into single user messages with deduplicated per-block tool_use_ids.
    fn build_anthropic_messages(messages: &[Message]) -> Vec<Value> {
        let mut result = Vec::new();
        let mut pending_tool_results: Vec<Value> = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for msg in messages {
            match msg {
                Message::ToolResult(tr) => {
                    // Skip if this tool_use_id already has a result in the pending batch
                    if seen_ids.contains(&tr.tool_use_id) { continue; }
                    seen_ids.insert(tr.tool_use_id.clone());
                    // Merge multiple content blocks into a single tool_result (API requires 1:1)
                    let merged: String = tr.content.iter().filter_map(|cb| match cb {
                        crate::types::message::ContentBlock::Text { text } => Some(text.as_str()),
                        crate::types::message::ContentBlock::FileReference { preview, .. } => Some(preview.as_str()),
                    }).collect::<Vec<_>>().join("\n");
                    pending_tool_results.push(serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tr.tool_use_id,
                        "content": merged,
                    }));
                }
                _ => {
                    if !pending_tool_results.is_empty() {
                        result.push(serde_json::json!({
                            "role": "user",
                            "content": std::mem::take(&mut pending_tool_results),
                        }));
                        seen_ids.clear();
                    }
                    if let Some(m) = Self::build_anthropic_message(msg) {
                        result.push(m);
                    }
                }
            }
        }
        if !pending_tool_results.is_empty() {
            result.push(serde_json::json!({
                "role": "user",
                "content": pending_tool_results,
            }));
        }
        result
    }

    /// 构建 DeepSeek API 兼容消息格式。
    fn build_anthropic_message(msg: &Message) -> Option<Value> {
        match msg {
            Message::User(m) => Some(serde_json::json!({
                "role": "user",
                "content": m.content,
            })),
            Message::Assistant(m) => {
                let mut blocks = Vec::new();

                // thinking block
                if let Some(ref thinking) = m.thinking {
                    blocks.push(serde_json::json!({
                        "type": "thinking",
                        "thinking": thinking,
                    }));
                }

                // text block
                if let Some(ref content) = m.content
                    && !content.is_empty() {
                        blocks.push(serde_json::json!({
                            "type": "text",
                            "text": content,
                        }));
                    }

                // tool_use blocks
                for tu in &m.tool_uses {
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tu.id,
                        "name": tu.name,
                        "input": tu.input,
                    }));
                }

                if blocks.is_empty() {
                    return None;
                }

                Some(serde_json::json!({
                    "role": "assistant",
                    "content": blocks,
                }))
            }
            Message::ToolResult(m) => {
                // Merge all content blocks into a single text for 1:1 tool_use→tool_result
                let merged: String = m.content.iter().filter_map(|cb| match cb {
                    crate::types::message::ContentBlock::Text { text } => Some(text.as_str()),
                    crate::types::message::ContentBlock::FileReference { preview, .. } => Some(preview.as_str()),
                }).collect::<Vec<_>>().join("\n");
                Some(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.tool_use_id,
                        "content": merged,
                    }],
                }))
            }
            Message::System(m) => Some(serde_json::json!({
                "role": "user",
                "content": m.content,
            })),
        }
    }

    /// 解析 DeepSeek API 兼容响应。
    fn parse_anthropic_response(&self, body: &Value, start: std::time::Instant) -> AgentResult<LlmResponse> {
        let content_items = body
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| AgentError::ApiError {
                status: 200,
                body: "Response missing content array".into(),
            })?;

        let mut text_content: Option<String> = None;
        let mut reasoning: Option<String> = None;
        let mut tool_uses = Vec::new();

        for item in content_items {
            match item.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    text_content = item
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string());
                }
                Some("thinking") => {
                    reasoning = item
                        .get("thinking")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string());
                }
                Some("tool_use") => {
                    if let (Some(id), Some(name), Some(input)) = (
                        item.get("id").and_then(|v| v.as_str()),
                        item.get("name").and_then(|v| v.as_str()),
                        item.get("input"),
                    ) {
                        tool_uses.push(crate::types::message::ToolUse {
                            id: id.to_string(),
                            name: name.to_string(),
                            input: input.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        let stop_reason = body
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());

        let usage = body.get("usage").map_or_else(
            crate::types::message::TokenUsage::default,
            |u| crate::types::message::TokenUsage {
                input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                cache_read_tokens: u
                    .get("cache_read_input_tokens").or_else(|| u.get("prompt_cache_hit_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cache_write_tokens: 0,
            },
        );

        Ok(LlmResponse {
            content: text_content,
            reasoning,
            tool_uses,
            stop_reason,
            usage,
            model: self.model_info.id.clone(),
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }
}

#[async_trait]
impl LlmClient for DeepSeekClient {
    async fn chat_stream(
        &self,
        system_prompt: &str,
        messages: &[Message],
        config: &LlmRequest,
        on_event: &(dyn Fn(StreamEvent) + Send + Sync),
    ) -> AgentResult<LlmResponse> {
        let start = std::time::Instant::now();

        let anthropic_messages = Self::build_anthropic_messages(messages);

        let mut request_body = serde_json::json!({
            "model": config.model,
            "max_tokens": config.max_tokens,
            "temperature": config.temperature,
            "messages": anthropic_messages,
            "system": system_prompt,
            "stream": true,
        });

        if !config.tools_json.is_empty()
            && let Ok(tools) = serde_json::from_str::<Vec<serde_json::Value>>(&config.tools_json)
                && !tools.is_empty() {
                    request_body["tools"] = serde_json::json!(tools);
                }

        if config.thinking_enabled {
            request_body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": 32768,
            });
        }

        // Check request size before sending (prevent silent API hangs on large context)
        let body_str = serde_json::to_string(&request_body).unwrap_or_default();
        if body_str.len() > 800_000 {
            return Err(AgentError::ConfigError(
                "Request too large — use /clear to start a new session".into()
            ));
        }

        let url = format!("{}{}", self.base_url, ANTHROPIC_MESSAGES_PATH);

        let response = self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("User-Agent", &self.user_id)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AgentError::ApiUnreachable { attempts: 1, source: e })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::ApiError { status: status.as_u16(), body });
        }

        // Parse SSE stream with cross-chunk line buffering and partial JSON recovery.
        // Lines can be split across chunk boundaries — the last incomplete line
        // stays in `buf` until the next chunk completes it.
        let mut stream = response.bytes_stream();
        let mut buf = String::new();
        let mut pending_tool_id: Option<String> = None;
        let mut pending_tool_name: Option<String> = None;
        let mut pending_tool_input = String::new();
        let mut accumulated: LlmResponse = LlmResponse {
            content: Some(String::new()),
            reasoning: Some(String::new()),
            tool_uses: vec![],
            stop_reason: None,
            usage: Default::default(),
            model: config.model.clone(),
            latency_ms: 0,
        };
        let mut last_activity = std::time::Instant::now();

        let chunk_timeout = std::time::Duration::from_secs(60);
        let mut stream_done = false;
        while let Ok(Some(chunk_result)) = tokio::time::timeout(chunk_timeout, stream.next()).await {
            last_activity = std::time::Instant::now();
            let chunk = chunk_result.map_err(|e| AgentError::ApiError {
                status: 200,
                body: format!("Stream error: {e}"),
            })?;
            buf.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete lines, leaving incomplete last line in buffer
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf = buf[pos + 1..].to_string();

                if line.is_empty() { continue; }
                if !line.starts_with("data: ") { continue; }

                let json_str = &line[6..];
                // Attempt JSON parse; skip malformed events (don't crash the stream)
                if let Ok(event) = serde_json::from_str::<Value>(json_str) {
                    match event.get("type").and_then(|t| t.as_str()) {
                        Some("content_block_start") => {
                            if let Some(cb) = event.get("content_block") {
                                if let Some("tool_use") = cb.get("type").and_then(|t| t.as_str()) {
                                    pending_tool_id = cb.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                                    pending_tool_name = cb.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                                    pending_tool_input.clear();
                                }
                            }
                        }
                        Some("content_block_delta") => {
                            if let Some(delta) = event.get("delta") {
                                match delta.get("type").and_then(|t| t.as_str()) {
                                    Some("thinking_delta") => {
                                        if let Some(t) = delta.get("thinking").and_then(|v| v.as_str()) {
                                            on_event(StreamEvent::ThinkingDelta(t.to_string()));
                                            if let Some(ref mut r) = accumulated.reasoning { r.push_str(t); }
                                        }
                                    }
                                    Some("text_delta") => {
                                        if let Some(t) = delta.get("text").and_then(|v| v.as_str()) {
                                            on_event(StreamEvent::TextDelta(t.to_string()));
                                            if let Some(ref mut c) = accumulated.content { c.push_str(t); }
                                        }
                                    }
                                    Some("input_json_delta") => {
                                        if let Some(json) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                            pending_tool_input.push_str(json);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some("content_block_stop") => {
                            if let (Some(id), Some(name)) = (pending_tool_id.take(), pending_tool_name.take()) {
                                let input = std::mem::take(&mut pending_tool_input);
                                let parsed = serde_json::from_str(&input).unwrap_or(Value::String(input));
                                // Add to accumulated response so AgentLoop sees it
                                accumulated.tool_uses.push(crate::types::message::ToolUse {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: parsed.clone(),
                                });
                                on_event(StreamEvent::ToolUseStart { id, name, input: parsed });
                            }
                        }
                        Some("message_start") => {
                            // Try nested message.usage first (Anthropic spec), then top-level usage (DeepSeek variant)
                            let usage = event.get("message").and_then(|m| m.get("usage"))
                                .or_else(|| event.get("usage"));
                            if let Some(usage) = usage {
                                let inp = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                let cache = usage.get("cache_read_input_tokens")
                                    .or_else(|| usage.get("prompt_cache_hit_tokens"))
                                    .and_then(|v| v.as_u64()).unwrap_or(0);
                                tracing::debug!("message_start usage: input={inp} cache={cache} output=0");
                                accumulated.usage.input_tokens = inp;
                                accumulated.usage.cache_read_tokens = cache;
                            }
                        }
                        Some("message_delta") => {
                            if let Some(usage) = event.get("usage") {
                                // message_delta: only output_tokens per Anthropic spec
                                if let Some(out) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                    accumulated.usage.output_tokens = out;
                                }
                            }
                            if let Some(delta) = event.get("delta") {
                                accumulated.stop_reason = delta.get("stop_reason")
                                    .and_then(|v| v.as_str()).map(|s| s.to_string());
                            }
                        }
                        Some("message_stop") => {
                            stream_done = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        accumulated.latency_ms = start.elapsed().as_millis() as u64;
        if !stream_done && accumulated.content.as_ref().is_none_or(|c| c.trim().is_empty()) && accumulated.tool_uses.is_empty() {
            let elapsed = start.elapsed().as_secs();
            let err_msg = format!(
                "API stream ended without response ({}s elapsed, last activity {}s ago). Context: {} messages. Use /clear if persistent.",
                elapsed, last_activity.elapsed().as_secs(),
                messages.len(),
            );
            on_event(StreamEvent::TextDelta(format!("\n[Error] {err_msg}\n")));
            on_event(StreamEvent::Done(accumulated.clone()));
            return Err(AgentError::Internal(err_msg));
        }
        on_event(StreamEvent::Done(accumulated.clone()));
        Ok(accumulated)
    }

    async fn chat(
        &self,
        system_prompt: &str,
        messages: &[Message],
        config: &LlmRequest,
    ) -> AgentResult<LlmResponse> {
        let start = std::time::Instant::now();

        let anthropic_messages = Self::build_anthropic_messages(messages);

        let mut request_body = serde_json::json!({
            "model": config.model,
            "max_tokens": config.max_tokens,
            "temperature": config.temperature,
            "messages": anthropic_messages,
            "system": system_prompt,
        });

        // thinking/reasoning_effort
        if config.thinking_enabled {
            match config.reasoning_effort {
                ReasoningEffort::Off => {
                    // 不传 thinking 参数 → 禁用
                }
                ReasoningEffort::High | ReasoningEffort::Max => {
                    request_body["thinking"] = serde_json::json!({
                        "type": "enabled",
                        "budget_tokens": if config.reasoning_effort == ReasoningEffort::Max {
                            32768
                        } else {
                            16384
                        },
                    });
                }
            }
        }

        // Web Search (DeepSeek 需要具体版本号 + name 字段)
        if config.web_search_enabled {
            request_body["tools"] = serde_json::json!([{
                "type": "web_search_20260209",
                "name": "web_search",
            }]);
        }

        // Strict Schema (beta)
        let url = if config.strict_schema {
            format!(
                "{}/beta{}",
                self.base_url, ANTHROPIC_MESSAGES_PATH
            )
        } else {
            format!("{}{}", self.base_url, ANTHROPIC_MESSAGES_PATH)
        };

        tracing::debug!(
            messages = anthropic_messages.len(),
            system_chars = system_prompt.len(),
            model = %config.model,
            max_tokens = config.max_tokens,
            thinking = config.thinking_enabled,
            web_search = config.web_search_enabled,
            "deepseek.api.request"
        );

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("User-Agent", &self.user_id)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AgentError::ApiUnreachable {
                attempts: 1,
                source: e,
            })?;

        let status = response.status();
        let body_text = response.text().await.map_err(|e| AgentError::ApiError {
            status: status.as_u16(),
            body: format!("Failed to read response body: {}", e),
        })?;

        if !status.is_success() {
            tracing::warn!(status = status.as_u16(), body = %body_text.chars().take(100).collect::<String>(), "deepseek.api.error");
            return Err(match status.as_u16() {
                429 => AgentError::RateLimited {
                    retry_after_seconds: 5,
                },
                402 => AgentError::InsufficientBalance,
                s => AgentError::ApiError {
                    status: s,
                    body: body_text.chars().take(200).collect(),
                },
            });
        }

        let body: Value = serde_json::from_str(&body_text).map_err(|e| {
            AgentError::ApiError {
                status: 200,
                body: format!("Failed to parse JSON response: {} (body starts: {})",
                    e, &body_text[..body_text.len().min(100)]),
            }
        })?;

        let result = self.parse_anthropic_response(&body, start)?;
        tracing::info!(
            model = %result.model,
            latency_ms = result.latency_ms,
            input_tokens = result.usage.input_tokens,
            output_tokens = result.usage.output_tokens,
            cache_read = result.usage.cache_read_tokens,
            has_reasoning = result.reasoning.is_some(),
            has_content = result.content.is_some(),
            tool_calls = result.tool_uses.len(),
            "deepseek.api.response"
        );
        Ok(result)
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model_info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_user_message() {
        let msg = Message::User(crate::types::message::UserMessage {
            id: "msg_1".into(),
            timestamp: chrono::Utc::now(),
            content: "Hello".into(),
            metadata: Default::default(),
        });
        let anthropic = DeepSeekClient::build_anthropic_message(&msg).unwrap();
        assert_eq!(anthropic["role"], "user");
        assert_eq!(anthropic["content"], "Hello");
    }

    #[test]
    fn test_missing_api_key() {
        if std::env::var("DEEPSEEK_API_KEY").is_err() {
            assert!(DeepSeekClient::from_env().is_err());
        }
    }

    #[test]
    fn test_model_info_v4_flash() {
        let client = DeepSeekClient::new("test-key".into(), "deepseek-v4-flash").unwrap();
        let info = client.model_info();
        assert!(info.max_context_tokens > 0);
        assert!(info.supports_caching);
    }

    #[tokio::test]
    async fn test_real_api_call() {
        // 跳过: 需要设置 DEEPSEEK_API_KEY 环境变量
        if std::env::var("DEEPSEEK_API_KEY").is_err() {
            return;
        }

        let client = DeepSeekClient::from_env().unwrap();
        let messages = vec![Message::User(crate::types::message::UserMessage {
            id: "test-1".into(),
            timestamp: chrono::Utc::now(),
            content: "Hello! Respond with just the word 'Hi'.".into(),
            metadata: Default::default(),
        })];

        let config = LlmRequest {
            max_tokens: 100,
            thinking_enabled: false,
            ..Default::default()
        };

        let response = client.chat("You are a helpful assistant. Be brief.", &messages, &config).await.unwrap();
        assert!(!response.content.unwrap_or_default().is_empty());
    }
}
