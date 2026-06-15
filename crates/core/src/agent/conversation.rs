use crate::types::message::{AssistantMessage, Message, TokenUsage};

/// 对话状态。Agent 循环每轮读写，上下文压缩触发于此，
/// 记忆系统的 Episode 创建依赖此模块提供的完整对话轨迹。
pub struct ConversationState {
    /// 完整对话历史 (System + User + Assistant + ToolResult)
    messages: Vec<Message>,
    /// 当前轮数 (每次 User 消息 +1)
    turn_count: u32,
    /// 累计 token 使用
    total_usage: TokenUsage,
    /// 总成本 (USD)
    total_cost_usd: f64,
    /// 会话开始时的时间戳
    started_at: chrono::DateTime<chrono::Utc>,
    /// 当前正在处理的工具调用 (并行执行中)
    pending_tools: Vec<String>,
}

impl ConversationState {
    /// 创建新的对话状态。预分配 200 条消息容量，100 轮 × 2 条/轮 = 200。
    pub fn new() -> Self {
        Self {
            messages: Vec::with_capacity(200),
            turn_count: 0,
            total_usage: TokenUsage::default(),
            total_cost_usd: 0.0,
            started_at: chrono::Utc::now(),
            pending_tools: Vec::new(),
        }
    }

    /// 添加一条消息。User 消息自动递增 turn_count。
    pub fn add_message(&mut self, msg: Message) {
        if matches!(msg, Message::User(_)) {
            self.turn_count += 1;
        }
        self.messages.push(msg);
    }

    /// 累加 token 使用和成本。
    /// `price_per_mtok` = 每百万 token 的美元价格。
    /// 成本 = (input × input_price + output × output_price) / 1_000_000
    pub fn add_cost(&mut self, usage: TokenUsage, price_per_mtok_input: f64, price_per_mtok_output: f64) {
        self.total_usage.input_tokens += usage.input_tokens;
        self.total_usage.output_tokens += usage.output_tokens;
        self.total_usage.cache_read_tokens += usage.cache_read_tokens;
        self.total_usage.cache_write_tokens += usage.cache_write_tokens;

        let cost = (usage.input_tokens as f64 * price_per_mtok_input
            + usage.output_tokens as f64 * price_per_mtok_output)
            / 1_000_000.0;
        self.total_cost_usd += cost;
    }

    /// 查找最后一个 AssistantMessage，用于检查 Agent 是否在等待工具结果。
    pub fn last_assistant_message(&self) -> Option<&AssistantMessage> {
        self.messages
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::Assistant(a) => Some(a),
                _ => None,
            })
    }

    /// 估计当前对话的 token 数。
    /// 粗略估算: 字符总数 ÷ 3 (1 token ≈ 3 英文字符, DeepSeek 官方)
    pub fn estimated_tokens(&self) -> usize {
        self.messages.iter().map(|m| m.estimated_char_len()).sum::<usize>() / 3
    }

    /// 是否需要上下文压缩。
    pub fn needs_compaction(&self, max_tokens: usize) -> bool {
        self.estimated_tokens() > max_tokens
    }

    // ── 访问器 ──

    /// 消息总数。
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// 当前轮数。
    pub fn turn_count(&self) -> u32 {
        self.turn_count
    }

    /// 累计总成本 (USD)。
    pub fn total_cost_usd(&self) -> f64 {
        self.total_cost_usd
    }

    /// 累计 token 使用。
    pub fn total_usage(&self) -> &TokenUsage {
        &self.total_usage
    }

    /// 会话开始时间。
    pub fn started_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.started_at
    }

    /// 所有消息的不可变引用。
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// 所有消息的可变引用。用于 ContextManager 折叠。
    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }

    /// 添加正在执行中的工具调用 ID。
    pub fn add_pending_tool(&mut self, tool_call_id: String) {
        self.pending_tools.push(tool_call_id);
    }

    /// 移除已完成的工具调用 ID。
    pub fn remove_pending_tool(&mut self, tool_call_id: &str) {
        self.pending_tools.retain(|id| id != tool_call_id);
    }

    /// 是否有正在执行的工具调用。
    pub fn has_pending_tools(&self) -> bool {
        !self.pending_tools.is_empty()
    }

    /// Clear the entire conversation. Resets the session while keeping the same agent.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.pending_tools.clear();
        self.total_usage = TokenUsage::default();
        self.total_cost_usd = 0.0;
    }

    /// Trim conversation back to just the original user message + system messages.
    /// Used for fresh-context retry: discard failed attempt context, keep only what the user asked.
    /// Returns the count of messages removed.
    pub fn trim_to_user_input(&mut self) -> usize {
        use crate::types::message::Message;
        let before = self.messages.len();

        // Keep: system messages at the start + the first user message
        // Discard: everything after (LLM responses, tool results, rescue prompts, etc.)
        if let Some(first_user_idx) = self.messages.iter().position(|m| matches!(m, Message::User(_))) {
            // Keep everything up to and including the first user message
            self.messages.truncate(first_user_idx + 1);
        }
        // If no user message found, keep everything (shouldn't happen in normal flow)

        self.pending_tools.clear();
        let removed = before - self.messages.len();
        if removed > 0 {
            tracing::info!("Fresh-context retry: trimmed {} messages ({before} → {})", removed, self.messages.len());
        }
        removed
    }
}

impl Default for ConversationState {
    fn default() -> Self {
        Self::new()
    }
}

impl Message {
    /// 估算消息的字符长度 (用于 token 估算)。
    /// 使用 serde_json 序列化得到近似字符数。
    pub fn estimated_char_len(&self) -> usize {
        // 快速路径: 用 serde 序列化估算
        serde_json::to_string(self).map(|s| s.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::{MessageMetadata, UserMessage};
    use chrono::Utc;

    fn make_user_msg(content: &str) -> Message {
        Message::User(UserMessage {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            content: content.to_string(),
            metadata: MessageMetadata::default(),
        })
    }

    fn make_assistant_msg(content: &str) -> Message {
        Message::Assistant(AssistantMessage {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            thinking: None,
            content: Some(content.to_string()),
            tool_uses: vec![],
            model: Some("deepseek-v4-flash".into()),
            usage: None,
            stop_reason: Some("end_turn".into()),
        })
    }

    #[test]
    fn test_new_conversation() {
        let conv = ConversationState::new();
        assert_eq!(conv.message_count(), 0);
        assert_eq!(conv.turn_count(), 0);
        assert_eq!(conv.total_cost_usd(), 0.0);
        assert!(!conv.has_pending_tools());
    }

    #[test]
    fn test_add_message_increments_turn_on_user() {
        let mut conv = ConversationState::new();
        conv.add_message(make_user_msg("hello"));
        assert_eq!(conv.turn_count(), 1);
        conv.add_message(make_assistant_msg("hi there"));
        assert_eq!(conv.turn_count(), 1); // Assistant 不增加
        conv.add_message(make_user_msg("another"));
        assert_eq!(conv.turn_count(), 2);
    }

    #[test]
    fn test_add_cost_accumulates() {
        let mut conv = ConversationState::new();
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_write_tokens: 0,
        };
        // DeepSeek V4 Flash: input ¥1/M = ~$0.14/M, output ¥2/M = ~$0.28/M
        conv.add_cost(usage, 0.14, 0.28);
        assert_eq!(conv.total_usage().input_tokens, 1000);
        assert_eq!(conv.total_usage().output_tokens, 500);
        // (1000*0.14 + 500*0.28) / 1M = 0.00028
        assert!((conv.total_cost_usd() - 0.00028).abs() < 0.00001);
    }

    #[test]
    fn test_last_assistant_message() {
        let mut conv = ConversationState::new();
        assert!(conv.last_assistant_message().is_none());

        conv.add_message(make_user_msg("q1"));
        conv.add_message(make_assistant_msg("a1"));
        conv.add_message(make_user_msg("q2"));
        conv.add_message(make_assistant_msg("a2"));

        let last = conv.last_assistant_message().unwrap();
        assert_eq!(last.content.as_deref(), Some("a2"));
    }

    #[test]
    fn test_needs_compaction() {
        let mut conv = ConversationState::new();
        // 消息很少，不需要压缩
        conv.add_message(make_user_msg("short"));
        assert!(!conv.needs_compaction(1_000_000));

        // 添加大量消息触发压缩
        let long_text = "x".repeat(10_000); // ~10K chars → ~3333 tokens
        for _ in 0..100 {
            conv.add_message(make_user_msg(&long_text));
        }
        assert!(conv.needs_compaction(50_000));
    }

    #[test]
    fn test_pending_tools() {
        let mut conv = ConversationState::new();
        assert!(!conv.has_pending_tools());

        conv.add_pending_tool("toolu_001".into());
        conv.add_pending_tool("toolu_002".into());
        assert!(conv.has_pending_tools());

        conv.remove_pending_tool("toolu_001");
        assert!(conv.has_pending_tools());

        conv.remove_pending_tool("toolu_002");
        assert!(!conv.has_pending_tools());
    }
}
