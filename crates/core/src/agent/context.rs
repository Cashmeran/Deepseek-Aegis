use crate::types::config::AgentConfig;
use crate::types::message::{Message, SystemMessage};

/// 折叠决策: ContextManager 根据当前状态返回的指令。
#[derive(Debug, Clone, PartialEq)]
pub enum FoldAction {
    /// 无需折叠。
    None,
    /// 执行折叠: tail_budget=保留的尾部token数, aggressive=是否激进模式。
    Fold {
        tail_budget: usize,
        aggressive: bool,
        ratio: f32,
    },
    /// 上下文严重溢出, 不再折叠, 强制总结退出。
    ExitWithSummary {
        ratio: f32,
        prompt_tokens: usize,
        ctx_max: usize,
    },
}

/// 折叠执行结果。
#[derive(Debug, Clone)]
pub struct FoldResult {
    pub folded: bool,
    pub before: usize,
    pub after: usize,
    pub summary_chars: usize,
}

/// 上下文管理器。负责 Cache-First 架构的缓存断点和多级自适应折叠。
///
/// 吸收 6级阈值:
///   75% → 正常折叠(保留20%尾部)
///   78% → 激进折叠(保留10%尾部)
///   80% → 强制总结退出
///   90% → 回合前预飞折叠
///   30% → 节省不足跳过折叠
///   15s → 折叠摘要硬超时
pub struct ContextManager {
    /// 系统提示 + 前N轮的缓存断点索引 (消息索引位置)
    cache_boundary_index: usize,
    /// 上次折叠后的消息数 (用于防止同一轮重复折叠)
    last_fold_count: usize,
    /// 配置引用 (折叠阈值来自此配置)
    config: AgentConfig,
}

impl ContextManager {
    /// 创建上下文管理器。
    /// `cache_boundary_index` = 缓存断点之前不变, 之后每轮更新 (默认 4 条消息边界)
    pub fn new(config: AgentConfig, cache_boundary_index: usize) -> Self {
        Self {
            cache_boundary_index,
            last_fold_count: 0,
            config,
        }
    }

    /// 将消息列表分为"可缓存前缀"和"动态后缀"。
    /// cache_breakpoint 控制后缀长度: 倒数 N 条消息在动态部分，其余在缓存部分。
    pub fn split_for_caching<'a>(
        &self,
        messages: &'a [Message],
        cache_breakpoint: usize,
    ) -> (&'a [Message], &'a [Message]) {
        let split_at = messages.len().saturating_sub(cache_breakpoint);
        messages.split_at(split_at)
    }

    /// 多级自适应折叠决策。
    /// `prompt_tokens` = 当前估算 token 数, `ctx_max` = 模型最大上下文
    /// `already_folded_this_turn` = 本轮已折叠过, 避免无限循环
    pub fn decide_fold_action(
        &self,
        prompt_tokens: usize,
        ctx_max: usize,
        already_folded_this_turn: bool,
    ) -> FoldAction {
        if ctx_max == 0 {
            return FoldAction::None;
        }
        let ratio = prompt_tokens as f32 / ctx_max as f32;

        // 80%+: 不再折叠, 强制总结退出
        if ratio > self.config.force_summary_threshold {
            return FoldAction::ExitWithSummary {
                ratio,
                prompt_tokens,
                ctx_max,
            };
        }

        // 已折叠过 → 本轮不再折叠 (防止无限折叠循环)
        if already_folded_this_turn {
            return FoldAction::None;
        }

        // 78%-80%: 激进折叠 (仅保留 10% 尾部)
        if ratio > self.config.fold_aggressive_threshold {
            return FoldAction::Fold {
                tail_budget: (ctx_max as f32 * self.config.fold_aggressive_tail_fraction)
                    as usize,
                aggressive: true,
                ratio,
            };
        }

        // 75%-78%: 正常折叠 (保留 20% 尾部)
        if ratio > self.config.fold_threshold {
            return FoldAction::Fold {
                tail_budget: (ctx_max as f32 * self.config.fold_tail_fraction) as usize,
                aggressive: false,
                ratio,
            };
        }

        FoldAction::None
    }

    /// 预飞检查: 回合开始前估算是否需要预折叠。
    /// > 90% ctx_max → 触发预折叠 (处理会话恢复/大粘贴场景)
    pub fn preflight_check(
        &self,
        estimated_tokens: usize,
        ctx_max: usize,
    ) -> Option<FoldAction> {
        if ctx_max == 0 {
            return None;
        }
        let ratio = estimated_tokens as f32 / ctx_max as f32;
        if ratio > self.config.turn_start_fold_threshold {
            return Some(FoldAction::Fold {
                tail_budget: (ctx_max as f32 * 0.25) as usize,
                aggressive: true,
                ratio,
            });
        }
        None
    }

    /// 执行折叠: 保留最近的 N 条消息 + 总结头部 → Skill/高优先级内容原样保留。
    /// `summary_fn` = 摘要生成函数 (通常是 LLM 调用, 由调用方注入)。
    /// `tail_budget` = 保留的尾部 token 预算。
    pub fn execute_fold(
        &mut self,
        messages: &mut Vec<Message>,
        tail_budget: usize,
        summary_fn: &(dyn Fn(&[Message]) -> String + Send + Sync),
    ) -> FoldResult {
        let before = messages.len();

        // 空对话不折叠
        if before == 0 {
            return FoldResult {
                folded: false,
                before: 0,
                after: 0,
                summary_chars: 0,
            };
        }

        // 粗略估算: 每条消息约 120 tokens。
        // 至少保留 1 条消息作为尾部锚点，防止 tail_budget=0 时清空全部消息。
        let keep_count = (tail_budget / 120).max(1);
        let keep_from = messages.len().saturating_sub(keep_count);
        let head: Vec<_> = messages.drain(0..keep_from).collect();

        // 提取 head 中必须原样保留的内容 (HIGH PRIORITY / User memory / Project memory)
        // 必须在 drain 之后调用: 只扫描被折叠的 head，避免尾部 pinned 内容重复注入
        let pinned = Self::extract_pinned_content(&head);

        // 节省不足 30% → 跳过折叠 (最小节省检查)
        if before > 0 && (head.len() as f32 / before as f32) < self.config.fold_min_savings_fraction {
            // 恢复被 drain 的消息
            let insert_pos = 0;
            messages.splice(insert_pos..insert_pos, head);
            self.last_fold_count = messages.len();
            return FoldResult {
                folded: false,
                before,
                after: before,
                summary_chars: 0,
            };
        }

        let summary = summary_fn(&head);

        // 前置: 折叠标记 + 固定内容 + 摘要
        messages.insert(
            0,
            Message::System(SystemMessage {
                content: format!(
                    "[CONVERSATION HISTORY SUMMARY — earlier turns folded]\n{pinned}{}",
                    if pinned.is_empty() { "" } else { "\n" }
                ) + &summary,
            }),
        );

        self.last_fold_count = messages.len();

        FoldResult {
            folded: true,
            before,
            after: messages.len(),
            summary_chars: summary.len(),
        }
    }

    /// 提取折叠中必须保留的内容。
    /// Skill Pin + Claude Code user/project memory 保护。
    fn extract_pinned_content(messages: &[Message]) -> String {
        let mut pinned = String::new();
        for msg in messages {
            if let Message::System(s) = msg
                && (s.content.starts_with("# HIGH PRIORITY")
                    || s.content.starts_with("# User memory")
                    || s.content.starts_with("# Project memory"))
                {
                    pinned.push_str(&s.content);
                    pinned.push('\n');
                }
        }
        pinned
    }

    /// 估算一组消息的 token 数 (粗略: 总字符数 ÷ 3)
    pub fn estimate_tokens(messages: &[Message]) -> usize {
        messages
            .iter()
            .map(|m| m.estimated_char_len())
            .sum::<usize>()
            / 3
    }

    // ── 访问器 ──

    pub fn cache_boundary_index(&self) -> usize {
        self.cache_boundary_index
    }

    pub fn set_cache_boundary_index(&mut self, index: usize) {
        self.cache_boundary_index = index;
    }

    pub fn last_fold_count(&self) -> usize {
        self.last_fold_count
    }

    /// 当前轮是否已经折叠过 (消息数与上次折叠后相同)
    pub fn already_folded_this_turn(&self, current_count: usize) -> bool {
        self.last_fold_count == current_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::{MessageMetadata, UserMessage};

    fn make_msg(content: &str) -> Message {
        Message::User(UserMessage {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            content: content.to_string(),
            metadata: MessageMetadata::default(),
        })
    }

    fn make_system_msg(content: &str) -> Message {
        Message::System(SystemMessage {
            content: content.to_string(),
        })
    }

    fn test_config() -> AgentConfig {
        AgentConfig::default()
    }

    #[test]
    fn test_split_for_caching() {
        let mgr = ContextManager::new(test_config(), 4);
        let msgs: Vec<_> = (0..10).map(|i| make_msg(&format!("msg {i}"))).collect();
        let (prefix, suffix) = mgr.split_for_caching(&msgs, 4);
        assert_eq!(prefix.len(), 6);
        assert_eq!(suffix.len(), 4);
    }

    #[test]
    fn test_split_for_caching_short_list() {
        let mgr = ContextManager::new(test_config(), 4);
        let msgs: Vec<_> = (0..2).map(|i| make_msg(&format!("msg {i}"))).collect();
        let (prefix, suffix) = mgr.split_for_caching(&msgs, 4);
        assert_eq!(prefix.len(), 0); // 不足4条时全部放入suffix
        assert_eq!(suffix.len(), 2);
    }

    #[test]
    fn test_decide_fold_none() {
        let mgr = ContextManager::new(test_config(), 4);
        // 使用率 50% → 不折叠
        let action = mgr.decide_fold_action(50_000, 100_000, false);
        assert_eq!(action, FoldAction::None);
    }

    #[test]
    fn test_decide_fold_normal() {
        let mgr = ContextManager::new(test_config(), 4);
        // 76% → 正常折叠 (75% threshold)
        let action = mgr.decide_fold_action(76_000, 100_000, false);
        match action {
            FoldAction::Fold { aggressive, .. } => assert!(!aggressive),
            _ => panic!("Expected Fold, got {:?}", action),
        }
    }

    #[test]
    fn test_decide_fold_aggressive() {
        let mgr = ContextManager::new(test_config(), 4);
        // 79% → 激进折叠 (78% threshold)
        let action = mgr.decide_fold_action(79_000, 100_000, false);
        match action {
            FoldAction::Fold { aggressive, .. } => assert!(aggressive),
            _ => panic!("Expected Fold, got {:?}", action),
        }
    }

    #[test]
    fn test_decide_fold_exit_with_summary() {
        let mgr = ContextManager::new(test_config(), 4);
        // 85% → 超过 80% → 强制退出
        let action = mgr.decide_fold_action(85_000, 100_000, false);
        assert!(matches!(action, FoldAction::ExitWithSummary { .. }));
    }

    #[test]
    fn test_already_folded_prevents_repeat() {
        let mgr = ContextManager::new(test_config(), 4);
        // 76% → 应该折叠, 但本轮已折叠 → 返回 None
        let action = mgr.decide_fold_action(76_000, 100_000, true);
        assert_eq!(action, FoldAction::None);
    }

    #[test]
    fn test_preflight_check_above_90() {
        let mgr = ContextManager::new(test_config(), 4);
        // 92% → 预飞折叠
        let action = mgr.preflight_check(92_000, 100_000);
        assert!(action.is_some());
        match action.unwrap() {
            FoldAction::Fold { aggressive, .. } => assert!(aggressive),
            _ => panic!("Expected Fold"),
        }
    }

    #[test]
    fn test_preflight_check_below_90() {
        let mgr = ContextManager::new(test_config(), 4);
        // 85% → 低于 90% 阈值 → 不触发
        let action = mgr.preflight_check(85_000, 100_000);
        assert!(action.is_none());
    }

    #[test]
    fn test_extract_pinned_content() {
        let msgs = vec![
            make_system_msg("# HIGH PRIORITY: never delete .git"),
            make_system_msg("normal system message"),
            make_system_msg("# User memory: prefer rustfmt"),
        ];
        let pinned = ContextManager::extract_pinned_content(&msgs);
        assert!(pinned.contains("HIGH PRIORITY"));
        assert!(pinned.contains("User memory"));
        assert!(!pinned.contains("normal system"));
    }

    #[test]
    fn test_execute_fold() {
        let mut mgr = ContextManager::new(test_config(), 4);
        let mut msgs: Vec<_> = (0..50)
            .map(|i| make_msg(&format!("message number {}", i)))
            .collect();

        let summary_fn = &|head: &[Message]| -> String {
            format!("Summarized {} messages", head.len())
        };

        let result = mgr.execute_fold(&mut msgs, 1000, summary_fn);
        assert!(result.folded);
        assert!(result.summary_chars > 0);
        assert!(msgs.len() < 50);
        // 第一条应该是折叠摘要
        match &msgs[0] {
            Message::System(s) => {
                assert!(s.content.contains("CONVERSATION HISTORY SUMMARY"));
                assert!(s.content.contains("Summarized"));
            }
            _ => panic!("First message should be System"),
        }
    }

    #[test]
    fn test_execute_fold_skips_when_savings_too_low() {
        let mut mgr = ContextManager::new(test_config(), 4);
        // 10条消息, tail_budget 足够容纳全部 → head 为空 → 无需折叠
        let mut msgs: Vec<_> = (0..10).map(|i| make_msg(&format!("msg {i}"))).collect();

        let summary_fn = &|head: &[Message]| -> String {
            format!("{} msgs", head.len())
        };

        // tail_budget 1200 tokens → keep_count = 10 → head=[] → 节省0% < 30%
        let result = mgr.execute_fold(&mut msgs, 1200, summary_fn);
        assert!(!result.folded);
        assert_eq!(msgs.len(), 10);
    }
}
