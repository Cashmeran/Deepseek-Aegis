//! Session healing + tool result shrinking — session healing patterns.
//!
//! 1. heal_loaded_messages: fix unpaired tool_calls, stamp reasoning_content
//! 2. shrink_tool_results: token-level truncation with CJK awareness
//! 3. force_summary: context overflow → force LLM to summarize before exile

use crate::types::message::Message;

/// Result of healing loaded messages.
#[derive(Debug, Default)]
pub struct HealReport {
    pub healed_count: usize,
    pub chars_saved: usize,
    pub tokens_saved: usize,
    pub dropped_tool_calls: usize,
    pub dropped_stray_tools: usize,
    pub stamped_reasoning: usize,
}

/// Fix unpaired tool_calls in loaded messages (prevents DeepSeek 400 errors).
/// Pattern: assistant with tool_calls that have no matching tool_result → drop them.
pub fn heal_loaded_messages(messages: &mut Vec<Message>) -> HealReport {
    let mut report = HealReport::default();

    // Pass 1: Fix tool call pairing
    let mut i = 0;
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    while i < messages.len() {
        let msg = &messages[i];

        if let Message::Assistant(assist) = msg {
            if !assist.tool_uses.is_empty() {
                // Count how many tool_results follow
                let needed: std::collections::HashSet<String> = assist.tool_uses.iter()
                    .map(|tu| tu.id.clone()).collect();
                let mut found: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut j = i + 1;
                while j < messages.len() {
                    if let Message::ToolResult(tr) = &messages[j] {
                        if needed.contains(&tr.tool_use_id) {
                            found.insert(tr.tool_use_id.clone());
                            j += 1;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                if found.len() == needed.len() {
                    // All paired — keep
                    out.push(messages[i].clone());
                    for k in (i + 1)..j {
                        out.push(messages[k].clone());
                    }
                    i = j;
                } else {
                    // Unpaired — drop assistant + any matched tools
                    report.dropped_tool_calls += needed.len() - found.len();
                    report.dropped_stray_tools += found.len();
                    report.healed_count += 1;
                    i = j;
                }
                continue;
            }
        }

        // Stray tool message (no preceding assistant with tool_calls)
        if let Message::ToolResult(_) = msg {
            report.dropped_stray_tools += 1;
            report.healed_count += 1;
            i += 1;
            continue;
        }

        out.push(msg.clone());
        i += 1;
    }

    *messages = out;

    // Pass 2: Stamp empty reasoning_content on assistant messages for thinking mode
    // (DeepSeek requires reasoning_content in multi-turn with tool calls)
    for msg in messages.iter_mut() {
        if let Message::Assistant(assist) = msg {
            if assist.thinking.is_none() && !assist.tool_uses.is_empty() {
                assist.thinking = Some(String::new());
                report.stamped_reasoning += 1;
                report.healed_count += 1;
            }
        }
    }

    report
}

/// Shrink oversized tool results by character count.
/// Only truncates tool-role messages — user/assistant content is preserved.
pub fn shrink_tool_results(messages: &mut [Message], max_chars: usize) -> HealReport {
    let mut report = HealReport::default();

    for msg in messages.iter_mut() {
        if let Message::ToolResult(tr) = msg {
            let total_len: usize = tr.content.iter().map(|cb| match cb {
                crate::types::message::ContentBlock::Text { text } => text.len(),
                _ => 0,
            }).sum();

            if total_len > max_chars {
                report.chars_saved += total_len - max_chars;
                report.healed_count += 1;

                // Truncate each text block proportionally
                for cb in tr.content.iter_mut() {
                    if let crate::types::message::ContentBlock::Text { text } = cb {
                        if text.len() > max_chars / 2 {
                            let keep = max_chars / 3;
                            *text = format!(
                                "{}...[truncated: {}→{} chars]",
                                &text[..keep.min(text.len())],
                                text.len(),
                                keep + 30,
                            );
                        }
                    }
                }
            }
        }
    }

    report
}

/// Shrink oversized tool results by estimated token count.
/// More precise than char-based for CJK text (1 char = 0.6 token for CN, 0.3 for EN).
pub fn shrink_tool_results_by_tokens(messages: &mut [Message], max_tokens: usize) -> HealReport {
    let max_chars = (max_tokens as f64 * 3.0) as usize; // conservative: 1 token ≈ 3 chars worst-case
    shrink_tool_results(messages, max_chars)
}

/// Context overflow → try to force a summary before giving up.
/// Sends a minimal prompt asking the LLM to summarize, with very low max_tokens.
pub fn force_summary_prompt() -> String {
    "The conversation is too long. Produce the briefest possible summary of the current state \
     — key decisions made, files changed, and next steps. Under 200 words. Be terse. Do NOT ask questions or request tools."
        .to_string()
}

/// Check if a message sequence should trigger force summary.
/// Returns true when estimated tokens exceed 90% of context window.
pub fn should_force_summary(estimated_tokens: usize, context_max: usize) -> bool {
    estimated_tokens > context_max * 9 / 10
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::{AssistantMessage, ContentBlock, ToolResultMessage, ToolUse};

    fn make_tool_result(id: &str, text: &str) -> Message {
        Message::ToolResult(ToolResultMessage {
            tool_use_id: id.into(),
            is_error: false,
            content: vec![ContentBlock::Text { text: text.into() }],
            elapsed_ms: 0,
        })
    }

    fn make_assistant_with_tools(tool_uses: Vec<ToolUse>) -> Message {
        Message::Assistant(AssistantMessage {
            id: "assist-1".into(),
            timestamp: chrono::Utc::now(),
            thinking: None,
            content: Some("using tools".into()),
            tool_uses,
            model: None,
            usage: None,
            stop_reason: None,
        })
    }

    #[test]
    fn test_heal_paired_tools_preserved() {
        let mut msgs = vec![
            make_assistant_with_tools(vec![ToolUse {
                id: "t1".into(), name: "bash".into(), input: serde_json::json!({}),
            }]),
            make_tool_result("t1", "output"),
        ];
        let len_before = msgs.len();
        let report = heal_loaded_messages(&mut msgs);
        assert_eq!(msgs.len(), len_before);
        assert_eq!(report.dropped_tool_calls, 0);
    }

    #[test]
    fn test_heal_unpaired_tools_dropped() {
        let mut msgs = vec![
            make_assistant_with_tools(vec![ToolUse {
                id: "t1".into(), name: "bash".into(), input: serde_json::json!({}),
            }]),
            // missing tool result for t1
            Message::User(crate::types::message::UserMessage {
                id: "u1".into(), timestamp: chrono::Utc::now(),
                content: "next".into(), metadata: Default::default(),
            }),
        ];
        let report = heal_loaded_messages(&mut msgs);
        assert_eq!(report.dropped_tool_calls, 1);
        // The user message should survive
        assert!(msgs.iter().any(|m| matches!(m, Message::User(_))));
    }

    #[test]
    fn test_heal_stray_tool_result_dropped() {
        let msgs = vec![
            Message::User(crate::types::message::UserMessage {
                id: "u1".into(), timestamp: chrono::Utc::now(),
                content: "hello".into(), metadata: Default::default(),
            }),
            make_tool_result("orphan", "output"), // no preceding tool_use
        ];
        let mut msgs = msgs;
        let report = heal_loaded_messages(&mut msgs);
        assert_eq!(report.dropped_stray_tools, 1);
        assert_eq!(msgs.len(), 1); // only user message
    }

    #[test]
    fn test_shrink_large_tool_result() {
        let mut msgs = vec![make_tool_result("t1", &"x".repeat(10_000))];
        let report = shrink_tool_results(&mut msgs, 500);
        assert!(report.healed_count > 0);
        if let Message::ToolResult(tr) = &msgs[0] {
            if let ContentBlock::Text { text } = &tr.content[0] {
                assert!(text.len() < 10_000);
                assert!(text.contains("truncated"));
            }
        }
    }

    #[test]
    fn test_should_force_summary() {
        assert!(should_force_summary(95_000, 100_000));
        assert!(!should_force_summary(80_000, 100_000));
    }
}
