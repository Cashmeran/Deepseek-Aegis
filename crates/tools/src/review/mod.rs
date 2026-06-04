//! review — Evaluator's structured assessment tool. Zero LLM cost.
//! Aggregates: SprintContract progress, CodeScorer score, git_diff, run_tests, constraints.
//! This is the third body (Evaluator) of the three-body harness.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct ReviewTool;

impl ToolMetadata for ReviewTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "review".into(),
            description: "Aggregate all verification results into a structured assessment report. The Evaluator's primary tool.".into(),
            prompt: "Use review as the final step before reporting completion.\n\
                     Aggregates ALL checks into one report:\n\
                     - SprintContract: tasks complete? acceptance met? constraints violated?\n\
                     - Code changes: what files were modified (from git_diff)\n\
                     - Tests: pass/fail with count (from run_tests)\n\
                     - CodeScorer: quality score\n\
                     - Verdict: PASS (all clear), WARN (advisory issues), FAIL (blocking issues)\n\
                     The Evaluator uses this to produce a single trusted assessment.\n\
                     After review, if WARN/FAIL → return to Generator with specific issues.\n\
                     If PASS → report to user with confidence.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "contract_progress": {
                        "type": "object",
                        "description": "SprintContract status: {done: N, total: N}",
                        "properties": {
                            "done": {"type": "integer"},
                            "total": {"type": "integer"}
                        }
                    },
                    "scorer_score": {"type": "number", "description": "CodeScorer score (0.0-1.0)"},
                    "files_changed": {"type": "array", "items": {"type": "string"}, "description": "Files modified (from git_diff)"},
                    "test_passed": {"type": "boolean", "description": "Did run_tests pass?"},
                    "test_output": {"type": "string", "description": "Key test output (truncated)"},
                    "blocking_issues": {"type": "array", "items": {"type": "string"}, "description": "Blocking issues found"},
                    "advisory_issues": {"type": "array", "items": {"type": "string"}, "description": "Advisory issues found"},
                    "constraints_check": {"type": "string", "description": "Any constraints violated? (empty = none)"}
                },
                "required": []
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for ReviewTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let blocking: Vec<&str> = tool_use.input.get("blocking_issues").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
        let advisory: Vec<&str> = tool_use.input.get("advisory_issues").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();

        let (done, total) = tool_use.input.get("contract_progress")
            .map(|p| (p.get("done").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                      p.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as usize))
            .unwrap_or((0, 0));

        let score = tool_use.input.get("scorer_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let test_passed = tool_use.input.get("test_passed").and_then(|v| v.as_bool()).unwrap_or(true);
        let files = tool_use.input.get("files_changed").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()).unwrap_or_default();
        let constraints = tool_use.input.get("constraints_check").and_then(|v| v.as_str()).unwrap_or("");

        let verdict = if !blocking.is_empty() || score < 0.4 { "FAIL" }
            else if !advisory.is_empty() || score < 0.7 || !test_passed { "WARN" }
            else if total > 0 && done < total { "WARN" }
            else { "PASS" };

        let mut report = format!("## Review Report — {}\n\n", match verdict { "PASS" => "PASS [PASS]", "WARN" => "WARN [WARN]️", _ => "FAIL [FAIL]" });

        if total > 0 {
            report.push_str(&format!("Tasks: {}/{} complete\n", done, total));
        }
        report.push_str(&format!("CodeScorer: {:.2}\n", score));
        report.push_str(&format!("Tests: {}\n", if test_passed { "PASS" } else { "FAIL" }));
        if !files.is_empty() {
            report.push_str(&format!("Files: {}\n", files.join(", ")));
        }
        if !constraints.is_empty() {
            report.push_str(&format!("Constraints: {}\n", constraints));
        }

        if !blocking.is_empty() {
            report.push_str("\n### BLOCKING\n");
            for b in &blocking { report.push_str(&format!("- {}\n", b)); }
        }
        if !advisory.is_empty() {
            report.push_str("\n### ADVISORY\n");
            for a in &advisory { report.push_str(&format!("- {}\n", a)); }
        }

        if verdict == "PASS" && blocking.is_empty() && advisory.is_empty() {
            report.push_str("\nAll checks passed. Ready to report completion.\n");
        } else if verdict == "WARN" {
            report.push_str("\nAdvisory issues found. Fix if practical, then re-review.\n");
        } else {
            report.push_str("\nBLOCKING issues must be fixed before reporting completion.\n");
        }

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: report }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_review_pass() {
        let tool = Arc::new(ReviewTool);
        let input = serde_json::json!({
            "contract_progress": {"done": 3, "total": 3},
            "scorer_score": 0.85,
            "test_passed": true,
            "files_changed": ["src/auth.rs"],
            "blocking_issues": [],
            "advisory_issues": []
        });
        let tu = ToolUse { id: "t1".into(), name: "review".into(), input };
        let ctx = ToolContext { working_dir: std::path::PathBuf::from("."), permission_mode: aegis_core::types::PermissionMode::Default, session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b { ContentBlock::Text { text } => Some(text.clone()), _ => None }).unwrap();
        assert!(text.contains("PASS"));
    }

    #[tokio::test]
    async fn test_review_fail() {
        let tool = Arc::new(ReviewTool);
        let input = serde_json::json!({
            "scorer_score": 0.3,
            "test_passed": false,
            "blocking_issues": ["SprintContract incomplete"],
            "advisory_issues": ["Missing tests"]
        });
        let tu = ToolUse { id: "t2".into(), name: "review".into(), input };
        let ctx = ToolContext { working_dir: std::path::PathBuf::from("."), permission_mode: aegis_core::types::PermissionMode::Default, session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b { ContentBlock::Text { text } => Some(text.clone()), _ => None }).unwrap();
        assert!(text.contains("FAIL"));
        assert!(text.contains("BLOCKING"));
    }
}
