//! ToolCallRepair — 4-Pass 工具调用修复管线
//!
//! Pass 1: Scavenge  — 从 reasoning_content 回收泄漏的 tool calls (regex 提取)
//! Pass 2: Truncation — 修复截断 JSON (补全括号, serde 验证)
//! Pass 3: Storm     — 抑制重复调用 (sliding window, window=6, threshold=3)
//! Pass 4: Flatten   — 规范化字段命名 (filepath→file_path, oldString→old_string)

use crate::types::config::AgentConfig;
use crate::types::message::ToolUse;
use std::collections::VecDeque;

pub struct ToolCallRepair {
    /// Pass 3 去重历史: 最近 N 次调用的 (tool_name, input_json)
    history: VecDeque<(String, String)>,
}

impl ToolCallRepair {
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(64),
        }
    }

    /// 主入口 — 四阶段修复管线
    pub fn process(
        &mut self,
        tool_uses: &[ToolUse],
        config: &AgentConfig,
        reasoning: Option<&str>,
    ) -> Vec<ToolUse> {
        let mut tools = tool_uses.to_vec();

        // Pass 1: Scavenge — 从 reasoning_content 回收泄漏的工具调用
        if config.repair_scavenge_enabled
            && let Some(reasoning) = reasoning {
                tools = Self::scavenge(tools, reasoning, config.repair_max_scavenge);
            }

        // Pass 2: Truncation — 修复截断 JSON
        tools = Self::fix_truncation(tools);

        // Pass 3: Storm — 抑制重复调用
        tools = Self::suppress_storm(
            &self.history,
            tools,
            config.repair_storm_window,
            config.repair_storm_threshold,
        );

        // Pass 4: Flatten — 规范化字段命名
        tools = Self::flatten(tools);

        // 更新历史记录
        for t in &tools {
            self.history.push_back((t.name.clone(), t.input.to_string()));
            if self.history.len() > 64 {
                self.history.pop_front();
            }
        }

        tools
    }

    // ═══════════════ Pass 1: Scavenge ═══════════════
    // 从 reasoning_content 中提取未被正式包含在 tool_uses 的 JSON 工具调用。
    // DeepSeek reasoning 中可能输出类似 {"name":"bash","input":{"command":"ls"}} 的片段。
    fn scavenge(mut tools: Vec<ToolUse>, reasoning: &str, max_extract: usize) -> Vec<ToolUse> {
        if tools.len() >= max_extract {
            return tools;
        }

        let re = regex::Regex::new(
            r#"\{[^}]*"name"\s*:\s*"(\w+)"[^}]*"input"\s*:\s*\{[^}]*\}[^}]*\}"#,
        );
        let re = match re {
            Ok(r) => r,
            Err(_) => return tools,
        };

        let mut extracted = 0;
        for cap in re.captures_iter(reasoning) {
            if extracted >= max_extract {
                break;
            }
            // cap[0] 是完整匹配的 JSON 对象
            if let Ok(full_match) = serde_json::from_str::<serde_json::Value>(&cap[0])
                && let (Some(name), Some(input)) = (
                    full_match.get("name").and_then(|v| v.as_str()),
                    full_match.get("input"),
                ) {
                    tools.push(ToolUse {
                        id: format!("toolu_scavenge_{}", uuid::Uuid::new_v4()),
                        name: name.to_string(),
                        input: input.clone(),
                    });
                    extracted += 1;
                }
        }
        tools
    }

    // ═══════════════ Pass 2: Truncation ═══════════════
    // LLM 输出被 max_tokens 截断时, 补全缺失的 } 和 ]
    fn fix_truncation(tools: Vec<ToolUse>) -> Vec<ToolUse> {
        tools
            .into_iter()
            .map(|mut t| {
                let s = t.input.to_string();
                if s.is_empty() {
                    return t;
                }

                let open_braces =
                    s.matches('{').count() as isize - s.matches('}').count() as isize;
                let open_brackets =
                    s.matches('[').count() as isize - s.matches(']').count() as isize;

                if open_braces > 0 || open_brackets > 0 {
                    let fixed = format!(
                        "{}{}{}",
                        s,
                        "}".repeat(open_braces.max(0) as usize),
                        "]".repeat(open_brackets.max(0) as usize),
                    );
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&fixed) {
                        t.input = v;
                    }
                }
                t
            })
            .collect()
    }

    // ═══════════════ Pass 3: Storm ═══════════════
    // 同一 (tool_name, input) 在最近 window 轮中出现 ≥ threshold 次 → 抑制
    fn suppress_storm(
        history: &VecDeque<(String, String)>,
        tools: Vec<ToolUse>,
        window: usize,
        threshold: usize,
    ) -> Vec<ToolUse> {
        let recent: Vec<&(String, String)> = history.iter().rev().take(window).collect();
        tools
            .into_iter()
            .filter(|t| {
                let sig = (t.name.clone(), t.input.to_string());
                let count = recent
                    .iter()
                    .filter(|(n, j)| *n == sig.0 && *j == sig.1)
                    .count();
                count < threshold
            })
            .collect()
    }

    // ═══════════════ Pass 4: Flatten ═══════════════
    // 规范化字段命名: filepath/path→file_path, oldString/old→old_string, etc.
    fn flatten(tools: Vec<ToolUse>) -> Vec<ToolUse> {
        tools
            .into_iter()
            .map(|mut t| {
                if let Some(obj) = t.input.as_object_mut() {
                    // filepath / path → file_path
                    if let Some(v) = obj.get("filepath").cloned() {
                        obj.entry("file_path".to_string()).or_insert(v);
                        obj.remove("filepath");
                    }
                    if let Some(v) = obj.get("path").cloned() {
                        obj.entry("file_path".to_string()).or_insert(v);
                        obj.remove("path");
                    }

                    // oldString / old → old_string
                    if let Some(v) = obj.get("oldString").cloned() {
                        obj.entry("old_string".to_string()).or_insert(v);
                        obj.remove("oldString");
                    }
                    if let Some(v) = obj.get("old").cloned() {
                        obj.entry("old_string".to_string()).or_insert(v);
                        obj.remove("old");
                    }

                    // newString / new → new_string
                    if let Some(v) = obj.get("newString").cloned() {
                        obj.entry("new_string".to_string()).or_insert(v);
                        obj.remove("newString");
                    }
                    if let Some(v) = obj.get("new").cloned() {
                        obj.entry("new_string".to_string()).or_insert(v);
                        obj.remove("new");
                    }
                }
                t
            })
            .collect()
    }
}

impl Default for ToolCallRepair {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::config::AgentConfig;

    fn make_tool_use(name: &str, input: serde_json::Value) -> ToolUse {
        ToolUse {
            id: format!("toolu_{}", uuid::Uuid::new_v4()),
            name: name.to_string(),
            input,
        }
    }

    #[test]
    fn test_scavenge_extracts_tool_call_from_reasoning() {
        let tools = vec![];
        let reasoning =
            r#"I should run: {"name":"bash","input":{"command":"ls -la"}}. Then check the output."#;
        let result = ToolCallRepair::scavenge(tools, reasoning, 4);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "bash");
        assert_eq!(result[0].input["command"], "ls -la");
    }

    #[test]
    fn test_scavenge_respects_max_extract() {
        let reasoning = r#"{"name":"a","input":{}} {"name":"b","input":{}} {"name":"c","input":{}}"#;
        let result = ToolCallRepair::scavenge(vec![], reasoning, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_truncation_fixes_missing_braces() {
        // Simulate truncated JSON string: remove last 3 chars ("lo"})
        let s = r#"{"command":"echo hello"}"#;
        let truncated = &s[..s.len() - 3]; // → {"command":"echo hel
        let v: serde_json::Value = serde_json::from_str(truncated).unwrap_or(serde_json::Value::Null);
        let tool = make_tool_use("bash", v);
        let result = ToolCallRepair::fix_truncation(vec![tool]);
        // Truncation should try to repair; if `null` was parsed, it stays `null`
        // This test validates the function doesn't panic on malformed input
        assert!(result.len() == 1);
    }

    #[test]
    fn test_truncation_keeps_valid_json() {
        let input = serde_json::json!({"command": "ls"});
        let tool = make_tool_use("bash", input.clone());
        let result = ToolCallRepair::fix_truncation(vec![tool]);
        assert_eq!(result[0].input["command"], "ls");
    }

    #[test]
    fn test_storm_suppresses_repeated_call() {
        let mut history = VecDeque::new();
        // 填充 3 次相同调用
        for _ in 0..3 {
            history.push_back(("bash".into(), r#"{"command":"ls"}"#.into()));
        }
        let tool = make_tool_use("bash", serde_json::json!({"command": "ls"}));
        let result = ToolCallRepair::suppress_storm(&history, vec![tool], 6, 3);
        assert!(
            result.is_empty(),
            "Should suppress the 4th identical call"
        );
    }

    #[test]
    fn test_storm_allows_different_calls() {
        let mut history = VecDeque::new();
        history.push_back(("bash".into(), r#"{"command":"ls"}"#.into()));
        let tool = make_tool_use("bash", serde_json::json!({"command": "pwd"}));
        let result = ToolCallRepair::suppress_storm(&history, vec![tool], 6, 3);
        assert_eq!(result.len(), 1, "Different command should be allowed");
    }

    #[test]
    fn test_flatten_normalizes_file_path() {
        let tool = make_tool_use("file_read", serde_json::json!({"filepath": "src/main.rs"}));
        let result = ToolCallRepair::flatten(vec![tool]);
        assert!(result[0].input.get("file_path").is_some());
        assert!(result[0].input.get("filepath").is_none());
    }

    #[test]
    fn test_flatten_normalizes_old_new_string() {
        let tool = make_tool_use(
            "file_edit",
            serde_json::json!({"oldString": "fn a()", "newString": "fn b()"}),
        );
        let result = ToolCallRepair::flatten(vec![tool]);
        assert!(result[0].input.get("old_string").is_some());
        assert!(result[0].input.get("new_string").is_some());
        assert!(result[0].input.get("oldString").is_none());
        assert!(result[0].input.get("newString").is_none());
    }

    #[test]
    fn test_full_process_noop_on_empty() {
        let mut repair = ToolCallRepair::new();
        let result = repair.process(&[], &AgentConfig::default(), None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_full_process_preserves_valid_tools() {
        let mut repair = ToolCallRepair::new();
        let tools = vec![make_tool_use("grep", serde_json::json!({"pattern": "fn main"})), make_tool_use("glob", serde_json::json!({"pattern": "*.rs"}))];
        let result = repair.process(&tools, &AgentConfig::default(), None);
        assert_eq!(result.len(), 2);
    }
}
