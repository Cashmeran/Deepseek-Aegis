//! run() — 非流式 Agent 主执行循环。

use crate::agent::helpers::{classify_task_type, is_complex_task};
use crate::agent::output::{AgentOutput, ConfidenceLevel};
use crate::agent::system_prompt::HarnessPhase;
use crate::error::AgentResult;
use crate::llm::client::LlmClient;
use crate::types::message::Message;
use crate::types::tool::TaskType;

use super::AgentLoop;

impl<L: LlmClient> AgentLoop<L> {
    /// 运行一次完整的 Agent 任务。
    /// 从用户输入开始，到返回最终回复或达到 max_turns 上限。
    pub async fn run(&mut self, user_input: &str) -> AgentResult<AgentOutput> {
        let session_id = self.conversation.started_at().to_rfc3339();
        tracing::info!(session_id, user_input = %user_input, "agent.run.start");

        self.conversation.add_message(Message::User(crate::types::message::UserMessage {
            id: format!("msg_{}", uuid::Uuid::new_v4()),
            timestamp: chrono::Utc::now(),
            content: user_input.to_string(),
            metadata: Default::default(),
        }));
        self.already_folded_this_turn = false;
        self.self_rescue_rounds = 0;

        if self.phase == HarnessPhase::Generator && is_complex_task(user_input) {
            self.phase = HarnessPhase::Planner;
        }

        let preflight = self.context_mgr.preflight_check(
            self.conversation.estimated_tokens(),
            self.effective_ctx_max(),
        );
        if let Some(crate::agent::context::FoldAction::Fold { tail_budget, .. }) = preflight {
            self.context_mgr.execute_fold(
                self.conversation.messages_mut(),
                tail_budget,
                &|head: &[Message]| {
                    format!(
                        "Summarized {} earlier messages — details omitted for brevity.",
                        head.len()
                    )
                },
            );
            self.already_folded_this_turn = true;
        }

        for turn in 0..self.config.max_turns {
            self.sync_config();
            self.advance_phase(turn);

            let fold_action = self.context_mgr.decide_fold_action(
                self.conversation.estimated_tokens(),
                self.effective_ctx_max(),
                self.already_folded_this_turn,
            );

            match fold_action {
                crate::agent::context::FoldAction::Fold { tail_budget, .. } => {
                    self.context_mgr.execute_fold(
                        self.conversation.messages_mut(),
                        tail_budget,
                        &|head: &[Message]| {
                            format!(
                                "Summarized {} earlier messages — details omitted for brevity.",
                                head.len()
                            )
                        },
                    );
                    self.already_folded_this_turn = true;
                }
                crate::agent::context::FoldAction::ExitWithSummary { .. } => {
                    return self.exit_with_summary().await;
                }
                crate::agent::context::FoldAction::None => {}
            }

            // P3: Flush pending steer at turn boundary (after previous turn settled)
            self.flush_steer();

            let response = match self.call_llm_with_recovery(user_input).await {
                Ok(r) => r,
                Err(e) => {
                    return Err(e);
                }
            };

            if !response.tool_uses.is_empty() {
                let repaired = self.repair.process(
                    &response.tool_uses,
                    &self.config,
                    response.reasoning.as_deref(),
                );
                self.try_auto_contract(&repaired);
                tracing::debug!(
                    original = response.tool_uses.len(),
                    repaired = repaired.len(),
                    "tool.repair.process"
                );
                let tool_results = self.execute_tools_parallel(&repaired).await?;
                let error_count = tool_results.iter().filter(|r| r.is_error).count();
                if error_count > 0 {
                    tracing::warn!(errors = error_count, total = tool_results.len(), "tool.errors");
                }
                self.conversation.add_message(Message::Assistant(crate::types::message::AssistantMessage {
                    id: format!("assist_{}", uuid::Uuid::new_v4()),
                    timestamp: chrono::Utc::now(),
                    thinking: response.reasoning.clone(),
                    content: response.content.clone(),
                    tool_uses: response.tool_uses.clone(),
                    model: Some(response.model.clone()),
                    usage: Some(response.usage),
                    stop_reason: response.stop_reason.clone(),
                }));
                self.conversation.add_cost(response.usage, self.llm.model_info().input_price_per_mtok, self.llm.model_info().output_price_per_mtok);
                for result in tool_results {
                    self.conversation.add_message(Message::ToolResult(result));
                }
                continue;
            }

            let content = response.content.unwrap_or_default();

            if self.phase == HarnessPhase::Generator && !content.trim().is_empty() {
                self.phase = HarnessPhase::Evaluator;
            }

            self.conversation
                .add_message(Message::Assistant(crate::types::message::AssistantMessage {
                    id: format!("assist_{}", uuid::Uuid::new_v4()),
                    timestamp: chrono::Utc::now(),
                    thinking: response.reasoning.clone(),
                    content: Some(content.clone()),
                    tool_uses: response.tool_uses.clone(),
                    model: Some(response.model.clone()),
                    usage: Some(response.usage),
                    stop_reason: response.stop_reason.clone(),
                }));

            self.conversation.add_cost(
                response.usage,
                self.llm.model_info().input_price_per_mtok,
                self.llm.model_info().output_price_per_mtok,
            );

            if !self.config.verify_before_output {
                return Ok(AgentOutput {
                    content,
                    confidence: ConfidenceLevel::Medium,
                    verification_report: None,
                    summary: None,
                });
            }

            let task_type = classify_task_type(user_input, &content);

            match task_type {
                TaskType::CodeGeneration | TaskType::CodeEdit => {
                    let verified = self.verify_output(&content).await?;

                    if !verified.passed {
                        self.self_rescue_rounds += 1;

                        if self.self_rescue_rounds > 8 {
                            let exhausted = self.self_rescue_rounds;
                            self.self_rescue_rounds = 0;
                            return Ok(AgentOutput {
                                content,
                                confidence: ConfidenceLevel::Low,
                                verification_report: Some(verified.report),
                                summary: Some(format!(
                                    "Self-rescue exhausted after {} rounds",
                                    exhausted
                                )),
                            });
                        }

                        let prompt = self.trigger_fresh_rescue(self.self_rescue_rounds, &verified.details);
                        self.conversation
                            .add_message(Message::System(crate::types::message::SystemMessage {
                                content: prompt,
                            }));
                        self.phase = HarnessPhase::Generator;
                        continue;
                    }

                    self.self_rescue_rounds = 0;
                    return Ok(AgentOutput {
                        content,
                        confidence: verified.confidence,
                        verification_report: Some(verified.report),
                        summary: None,
                    });
                }
                TaskType::Question | TaskType::Conversation => {
                    return Ok(AgentOutput {
                        content,
                        confidence: ConfidenceLevel::Medium,
                        verification_report: None,
                        summary: None,
                    });
                }
            }
        }

        self.exit_with_summary().await
    }
}
