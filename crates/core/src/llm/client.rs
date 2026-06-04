use crate::error::AgentResult;
use crate::types::message::Message;
use crate::types::tool::ReasoningEffort;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 流式事件 — chat_stream 回调的每个 token/thinking/tool 块
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 文本块 (可累积显示)
    TextDelta(String),
    /// Thinking 块 (灰色斜体)
    ThinkingDelta(String),
    /// 工具调用出现 (含参数)
    ToolUseStart { id: String, name: String, input: serde_json::Value },
    /// 工具执行完成
    ToolResult { id: String, name: String, is_error: bool, output: String, elapsed_ms: u64 },
    /// Agent 请求用户输入 (ask_user 工具)
    AskUser { question: String, header: String, options: Vec<AskOption> },
    /// 工具实时输出行 (bash 逐行流式输出)
    ToolProgress { tool_use_id: String, line: String },
    /// 流式结束, 包含完整响应
    Done(LlmResponse),
}

#[derive(Debug, Clone)]
pub struct AskOption {
    pub label: String,
    pub description: String,
}

/// LLM 客户端抽象。所有 LLM Provider 实现此 trait。
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发送消息列表, 返回 LLM 响应。
    async fn chat(&self, system_prompt: &str, messages: &[Message], config: &LlmRequest) -> AgentResult<LlmResponse>;

    /// 流式调用 LLM — 每个 token/thinking/tool 通过回调推送。
    /// 默认实现: 调用 chat 后一次性推送 (非真流式, 向后兼容)。
    async fn chat_stream(
        &self,
        system_prompt: &str,
        messages: &[Message],
        config: &LlmRequest,
        on_event: &(dyn Fn(StreamEvent) + Send + Sync),
    ) -> AgentResult<LlmResponse> {
        let result = self.chat(system_prompt, messages, config).await?;
        if let Some(ref t) = result.reasoning { on_event(StreamEvent::ThinkingDelta(t.clone())); }
        if let Some(ref c) = result.content { on_event(StreamEvent::TextDelta(c.clone())); }
        for tu in &result.tool_uses { on_event(StreamEvent::ToolUseStart { id: tu.id.clone(), name: tu.name.clone(), input: tu.input.clone() }); }
        on_event(StreamEvent::Done(result.clone()));
        Ok(result)
    }

    /// 返回此 Provider 的模型信息。
    fn model_info(&self) -> &ModelInfo;

    /// 估算给定消息列表的 token 数 (Provider 特定, 默认字符数÷3)。
    fn estimate_tokens(&self, messages: &[Message]) -> usize {
        messages.iter().map(|m| m.estimated_char_len()).sum::<usize>() / 3
    }
}

/// 单次 LLM 调用的请求配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    /// 模型名称, e.g. "deepseek-v4-flash"
    pub model: String,
    /// 最大输出 token 数
    pub max_tokens: u32,
    /// 采样温度 (0.0-2.0), 0=确定性输出
    pub temperature: f32,
    /// 推理强度 (仅 DeepSeek 支持)
    pub reasoning_effort: ReasoningEffort,
    /// 请求超时
    pub timeout: Duration,
    /// 用户标识 (DeepSeek KVCache 隔离)
    pub user_id: String,
    /// 是否启用 thinking 模式
    pub thinking_enabled: bool,
    /// 是否使用 Strict Schema (beta endpoint)
    pub strict_schema: bool,
    /// 是否启用 Web Search (DeepSeek 服务端处理, 无需客户端工具)
    pub web_search_enabled: bool,
    /// Anthropic 格式的工具定义 JSON (传给 API)
    pub tools_json: String,
}

impl Default for LlmRequest {
    fn default() -> Self {
        Self {
            model: "deepseek-v4-pro".into(),
            max_tokens: 393_216,  // 384K
            temperature: 0.0,
            reasoning_effort: ReasoningEffort::Max,
            timeout: Duration::from_secs(180),
            user_id: "deepseek-aegis".into(),
            thinking_enabled: true,
            strict_schema: false,
            web_search_enabled: true,
            tools_json: String::new(),
        }
    }
}

/// 单次 LLM 调用的响应。
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// 文本回复内容 (可为空, 当仅有 thinking 时)
    pub content: Option<String>,
    /// 推理链 (DeepSeek reasoning_content, 非 Anthropic thinking block)
    pub reasoning: Option<String>,
    /// 工具调用请求 (空=纯文本回复)
    pub tool_uses: Vec<crate::types::message::ToolUse>,
    /// 停止原因
    pub stop_reason: Option<String>,
    /// Token 使用统计
    pub usage: crate::types::message::TokenUsage,
    /// 模型名称 (实际使用的模型)
    pub model: String,
    /// 延迟 (毫秒)
    pub latency_ms: u64,
}

/// 模型元信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// 模型 ID, e.g. "deepseek-v4-flash"
    pub id: String,
    /// Provider 名称
    pub provider: String,
    /// 最大上下文 token 数
    pub max_context_tokens: usize,
    /// 最大输出 token 数
    pub max_output_tokens: u32,
    /// 输入价格 (USD / 百万 token)
    pub input_price_per_mtok: f64,
    /// 输出价格 (USD / 百万 token)
    pub output_price_per_mtok: f64,
    /// 缓存命中价格 (USD / 百万 token)
    pub cache_price_per_mtok: f64,
    /// 是否支持 reasoning/thinking
    pub supports_reasoning: bool,
    /// 是否支持 prompt caching
    pub supports_caching: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_request_default() {
        let req = LlmRequest::default();
        assert_eq!(req.model, "deepseek-v4-pro");
        assert_eq!(req.max_tokens, 393_216);
        assert_eq!(req.temperature, 0.0);
        assert!(req.thinking_enabled);
        assert!(!req.strict_schema);
        assert!(req.web_search_enabled);
    }

    #[test]
    fn test_model_info_deepseek_v4_flash() {
        let info = ModelInfo {
            id: "deepseek-v4-flash".into(),
            provider: "deepseek".into(),
            max_context_tokens: 1_048_576, // 1M
            max_output_tokens: 8192,
            input_price_per_mtok: 0.14,   // ~¥1/M
            output_price_per_mtok: 0.28,  // ~¥2/M
            cache_price_per_mtok: 0.0028, // ~¥0.02/M (1/50)
            supports_reasoning: true,
            supports_caching: true,
        };
        assert_eq!(info.provider, "deepseek");
        assert!(info.supports_caching);
    }
}
