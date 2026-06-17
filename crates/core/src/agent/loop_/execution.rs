//! Parallel tool execution and SprintContract auto-creation.

use crate::error::{AgentError, AgentResult};
use crate::llm::client::LlmClient;
use crate::types::message::{ContentBlock, ToolResultMessage, ToolUse};
use crate::types::tool::{ConcurrencySafety, ToolContext};
use std::sync::Arc;

use super::{AgentLoop, TodoStatus};

impl<L: LlmClient> AgentLoop<L> {
    /// After tool execution, check if a plan tool was called → auto-create SprintContract.
    pub(crate) fn try_auto_contract(&mut self, tool_uses: &[ToolUse]) {
        for tu in tool_uses {
            if tu.name == "plan"
                && let Some(contract) = crate::agent::harness::SprintContract::from_plan_json(&tu.input) {
                    tracing::info!("SprintContract auto-created from plan tool: {}", contract.objective);
                    self.set_contract(contract);
                    if self.phase() == crate::agent::system_prompt::HarnessPhase::Planner {
                        self.set_phase(crate::agent::system_prompt::HarnessPhase::Generator);
                    }
                }
            if tu.name == "todo_write"
                && let Some(ref mut contract) = self.active_contract
                    && let Some(tasks) = tu.input.get("tasks").and_then(|v| v.as_array()) {
                        let todos: Vec<(String, TodoStatus)> = tasks.iter().map(|t| {
                            let subject = t.get("subject").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let status = match t.get("status").and_then(|v| v.as_str()).unwrap_or("pending") {
                                "completed" => TodoStatus::Completed,
                                "in_progress" => TodoStatus::InProgress,
                                _ => TodoStatus::Pending,
                            };
                            (subject, status)
                        }).collect();
                        contract.sync_todos(&todos);
                    }
        }
    }

    /// 并行执行工具调用。执行前检查权限: Deny → 返回错误, Ask → 弹出确认, Allow → 执行。
    pub(crate) async fn execute_tools_parallel(
        &self,
        tool_uses: &[ToolUse],
    ) -> AgentResult<Vec<ToolResultMessage>> {
        if tool_uses.is_empty() {
            return Ok(vec![]);
        }

        let permission_mode = self.mode.to_permission_mode();
        let progress_tx: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>> =
            self.tool_progress_tx.as_ref().map(|tx| tx.clone());

        let ctx = ToolContext {
            working_dir: self.config.workspace_dir.clone().into(),
            permission_mode,
            session_id: self.conversation.started_at().to_rfc3339(),
            env: std::collections::HashMap::new(),
            sandbox_enabled: self.config.sandbox_backend != "none",
            sandbox: self.sandbox.clone(),
            timeout_ms: self.config.default_tool_timeout_ms,
            ask_user_cb: crate::types::tool::DebugAskUserCb(self.ask_user.clone()),
            progress_tx,
        };

        let max_parallel = self.config.max_parallel_tools.max(1);
        let mut results = Vec::with_capacity(tool_uses.len());

        for chunk in tool_uses.chunks(max_parallel) {
            let mut handles = Vec::with_capacity(chunk.len());
            for tu in chunk {
                // ── Permission check ──
                let perm = crate::tool_system::ToolPermissionChecker::check(
                    &tu.name, permission_mode,
                );
                match perm {
                    None => {
                        // Denied — return error immediately
                        let mode_name = format!("{:?}", permission_mode);
                        results.push(ToolResultMessage {
                            tool_use_id: tu.id.clone(),
                            is_error: true,
                            content: vec![ContentBlock::Text {
                                text: format!(
                                    "Permission denied: '{}' is not allowed in {} mode. Switch to Default or Yolo to use this tool.",
                                    tu.name, mode_name
                                ),
                            }],
                            elapsed_ms: 0,
                        });
                        continue;
                    }
                    Some(true) => {
                        // Needs approval — ask user via callback
                        if let Some(ref cb) = self.ask_user {
                            let question = serde_json::json!({
                                "questions": [{
                                    "question": format!("Allow tool call: {}?", tu.name),
                                    "header": "Permission",
                                    "options": [
                                        {"label": "Allow", "description": "Run this tool once"},
                                        {"label": "Deny", "description": "Skip this call"},
                                    ]
                                }]
                            });
                            let answer = cb(&question.to_string(), "Permission");
                            if answer.contains("Deny") || answer.is_empty() {
                                results.push(ToolResultMessage {
                                    tool_use_id: tu.id.clone(),
                                    is_error: true,
                                    content: vec![ContentBlock::Text {
                                        text: format!("User denied '{}'.", tu.name),
                                    }],
                                    elapsed_ms: 0,
                                });
                                continue;
                            }
                        }
                        // User approved → fall through to execute
                    }
                    Some(false) => {
                        // Auto-allowed → fall through to execute
                    }
                }

                if tu.name.to_lowercase() == "ask_user" {
                    if let Some(ref cb) = self.ask_user {
                        let input_json = serde_json::to_string(&tu.input).unwrap_or_default();
                        let header = tu.input.get("questions")
                            .and_then(|v| v.as_array())
                            .and_then(|a| a.first())
                            .and_then(|q| q.get("header").and_then(|v| v.as_str()))
                            .unwrap_or("Question");
                        let answer = cb(&input_json, header);
                        results.push(ToolResultMessage {
                            tool_use_id: tu.id.clone(),
                            is_error: false,
                            content: vec![ContentBlock::Text { text: answer }],
                            elapsed_ms: 0,
                        });
                        continue;
                    } else {
                        results.push(ToolResultMessage {
                            tool_use_id: tu.id.clone(),
                            is_error: true,
                            content: vec![ContentBlock::Text {
                                text: "BUG: ask_user callback is NOT set in AgentLoop. TUI must call with_ask_user().".into()
                            }],
                            elapsed_ms: 0,
                        });
                        continue;
                    }
                }

                let is_safe = self.registry.concurrency_safety_of(&tu.name) == ConcurrencySafety::ConcurrentSafe;
                if is_safe {
                    let registry = Arc::clone(&self.registry);
                    let tu = tu.clone();
                    let ctx = ctx.clone();
                    let handle = tokio::spawn(async move {
                        let result = registry.execute(&tu, &ctx).await;
                        (tu.id.clone(), result)
                    });
                    handles.push(handle);
                } else {
                    let result = self.registry.execute(tu, &ctx).await;
                    match result {
                        Ok(r) => results.push(r),
                        Err(e) => results.push(ToolResultMessage {
                            tool_use_id: tu.id.clone(), is_error: true,
                            content: vec![ContentBlock::Text {
                                text: format!("Tool error: {}", e),
                            }],
                            elapsed_ms: 0,
                        }),
                    }
                }
            }

            for handle in handles {
            match tokio::time::timeout(std::time::Duration::from_secs(30), handle).await {
                Ok(Ok((_id, Ok(result)))) => results.push(result),
                Ok(Ok((_id, Err(e)))) => {
                    results.push(ToolResultMessage {
                        tool_use_id: _id, is_error: true,
                        content: vec![ContentBlock::Text {
                            text: format!("Tool error: {}", e),
                        }],
                        elapsed_ms: 0,
                    });
                }
                Ok(Err(join_err)) => {
                    return Err(AgentError::Internal(format!("Tool task panicked: {}", join_err)));
                }
                Err(_elapsed) => {
                    results.push(ToolResultMessage {
                        tool_use_id: String::new(), is_error: true,
                        content: vec![ContentBlock::Text {
                            text: "Tool timed out (30s)".into(),
                        }],
                        elapsed_ms: 30_000,
                    });
                }
            }
            }
        }

        Ok(results)
    }
}
