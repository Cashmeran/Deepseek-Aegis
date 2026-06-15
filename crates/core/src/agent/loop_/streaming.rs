//! run_streaming() — 流式 Agent 执行循环，每个 LLM 响应块通过 callback 推送。

use crate::agent::helpers::classify_task_type;
use crate::agent::output::{AgentOutput, ConfidenceLevel};
use crate::agent::system_prompt::HarnessPhase;
use crate::error::{AgentError, AgentResult};
use crate::llm::client::LlmClient;
use crate::types::message::Message;
use crate::types::tool::TaskType;

use super::AgentLoop;

impl<L: LlmClient> AgentLoop<L> {
    /// 流式运行 — 每个 LLM 响应块通过 callback 推送。
    pub async fn run_streaming(
        &mut self,
        user_input: &str,
        on_stream: &(dyn Fn(crate::llm::client::StreamEvent) + Send + Sync),
    ) -> AgentResult<AgentOutput> {
        self.conversation.add_message(Message::User(crate::types::message::UserMessage {
            id: format!("msg_{}", uuid::Uuid::new_v4()),
            timestamp: chrono::Utc::now(),
            content: user_input.to_string(),
            metadata: Default::default(),
        }));
        self.inject_memory_message(user_input);
        self.already_folded_this_turn = false;
        self.self_rescue_rounds = 0;

        let (prog_tx, prog_rx) = std::sync::mpsc::channel::<String>();
        self.tool_progress_tx = Some(std::sync::Arc::new(move |line: String| {
            let _ = prog_tx.send(line);
        }));

        let preflight = self.context_mgr.preflight_check(self.conversation.estimated_tokens(), self.effective_ctx_max());
        if let Some(crate::agent::context::FoldAction::Fold { tail_budget, .. }) = preflight {
            self.context_mgr.execute_fold(self.conversation.messages_mut(), tail_budget,
                &|head: &[Message]| format!("Summarized {} earlier messages.", head.len()));
            self.already_folded_this_turn = true;
        }

        if self.mode == crate::types::tool::ExecutionMode::Plan {
            self.phase = HarnessPhase::Planner;
            self.planner_turns = 0;
        } else if self.phase != HarnessPhase::Evaluator {
            self.phase = HarnessPhase::Generator;
        }

        for turn in 0..self.config.max_turns {
            self.sync_config();
            self.advance_phase(turn);

            let est_before_shrink = self.conversation.estimated_tokens();
            let eff_max = self.effective_ctx_max();
            if est_before_shrink > eff_max * 3 / 4 {
                crate::agent::healing::shrink_tool_results_by_tokens(
                    self.conversation.messages_mut(),
                    eff_max / 20,
                );
            }

            let fold_action = self.context_mgr.decide_fold_action(self.conversation.estimated_tokens(), eff_max, self.already_folded_this_turn);
            match fold_action {
                crate::agent::context::FoldAction::Fold { tail_budget, .. } => {
                    self.context_mgr.execute_fold(self.conversation.messages_mut(), tail_budget,
                        &|head: &[Message]| format!("Summarized {} earlier messages.", head.len()));
                    self.already_folded_this_turn = true;
                }
                crate::agent::context::FoldAction::ExitWithSummary { .. } => return self.exit_with_summary().await,
                crate::agent::context::FoldAction::None => {}
            }

            let est = self.conversation.estimated_tokens();
            let eff_max = self.effective_ctx_max();
            if est > eff_max * 9 / 10 {
                return self.exit_with_summary().await;
            }

            // P3: Flush pending steer at turn boundary
            self.flush_steer();

            let system = self.build_system_prompt(user_input);
            let messages = self.conversation.messages();
            let tools_json = self.registry.get_anthropic_tools_json();
            let request = crate::llm::client::LlmRequest {
                model: self.config.default_model.clone(),
                max_tokens: 393_216,
                temperature: 0.0,
                reasoning_effort: crate::agent::helpers::parse_reasoning_effort(&self.config.reasoning_effort),
                timeout: std::time::Duration::from_secs(120),
                user_id: self.config.user_id.clone(),
                thinking_enabled: self.config.thinking_enabled,
                strict_schema: self.config.strict_tool_schema,
                web_search_enabled: self.config.web_search_enabled,
                tools_json,
            };

            let llm_start = std::time::Instant::now();
            let response = match tokio::time::timeout(
                std::time::Duration::from_secs(600),
                self.llm.chat_stream(&system, messages, &request, on_stream),
            ).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return Err(e),
                Err(_elapsed) => {
                    let waited = llm_start.elapsed().as_secs();
                    on_stream(crate::llm::client::StreamEvent::TextDelta(
                        format!("\n[Error] LLM call timed out after {waited}s. Context may be too large — use /clear.\n")
                    ));
                    return Err(AgentError::Internal(
                        format!("LLM call timed out after {waited}s")
                    ));
                }
            };

            if !response.tool_uses.is_empty() {
                let repaired = self.repair.process(
                    &response.tool_uses,
                    &self.config,
                    response.reasoning.as_deref(),
                );
                self.try_auto_contract(&repaired);
                let tool_results = match tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    self.execute_tools_parallel(&repaired),
                ).await {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        on_stream(crate::llm::client::StreamEvent::TextDelta(
                            "\n[Error] Tool execution timed out after 60s.\n".into()
                        ));
                        return Err(AgentError::Internal("Tool execution timed out".into()));
                    }
                };
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
                for result in &tool_results {
                    let output = result.content.iter().filter_map(|cb| match cb {
                        crate::types::message::ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    }).collect::<Vec<_>>().join("\n");
                    let name = repaired.iter()
                        .find(|tu| tu.id == result.tool_use_id)
                        .map(|tu| tu.name.clone())
                        .unwrap_or_default();
                    on_stream(crate::llm::client::StreamEvent::ToolResult {
                        id: result.tool_use_id.clone(),
                        name,
                        is_error: result.is_error,
                        output,
                        elapsed_ms: result.elapsed_ms,
                    });
                }
                while let Ok(line) = prog_rx.try_recv() {
                    on_stream(crate::llm::client::StreamEvent::ToolProgress {
                        tool_use_id: String::new(),
                        line,
                    });
                }
                for result in tool_results { self.conversation.add_message(Message::ToolResult(result)); }
                continue;
            }

            let content = response.content.unwrap_or_default();

            if self.phase == HarnessPhase::Generator && !content.trim().is_empty() {
                self.phase = HarnessPhase::Evaluator;
            }

            if let Some(ref contract) = self.active_contract {
                if !contract.acceptance_criteria.is_empty() {
                    match self.check_goal_completed(&content).await {
                        Some(ref answer) if answer == "YES" => {
                            on_stream(crate::llm::client::StreamEvent::TextDelta(
                                "\n\n[PASS]  Goal achieved! All acceptance criteria met.".into()
                            ));
                            self.conversation.add_message(Message::Assistant(crate::types::message::AssistantMessage {
                                id: format!("assist_{}", uuid::Uuid::new_v4()),
                                timestamp: chrono::Utc::now(),
                                thinking: response.reasoning.clone(),
                                content: Some(content.clone()),
                                tool_uses: response.tool_uses.clone(),
                                model: Some(response.model.clone()),
                                usage: Some(response.usage),
                                stop_reason: response.stop_reason.clone(),
                            }));
                            self.conversation.add_cost(response.usage, self.llm.model_info().input_price_per_mtok, self.llm.model_info().output_price_per_mtok);
                            return Ok(AgentOutput { content, confidence: ConfidenceLevel::High, verification_report: None, summary: Some("Goal achieved".into()) });
                        }
                        _ => {}
                    }
                }
            }

            self.conversation.add_message(Message::Assistant(crate::types::message::AssistantMessage {
                id: format!("assist_{}", uuid::Uuid::new_v4()),
                timestamp: chrono::Utc::now(),
                thinking: response.reasoning.clone(),
                content: Some(content.clone()),
                tool_uses: response.tool_uses.clone(),
                model: Some(response.model.clone()),
                usage: Some(response.usage),
                stop_reason: response.stop_reason.clone(),
            }));
            self.conversation.add_cost(response.usage, self.llm.model_info().input_price_per_mtok, self.llm.model_info().output_price_per_mtok);

            if !self.config.verify_before_output {
                return Ok(AgentOutput { content, confidence: ConfidenceLevel::Medium, verification_report: None, summary: None });
            }

            let task_type = classify_task_type(user_input, &content);
            match task_type {
                TaskType::CodeGeneration | TaskType::CodeEdit => {
                    let verified = self.verify_output(&content).await?;
                    if !verified.passed {
                        self.self_rescue_rounds += 1;
                        if self.self_rescue_rounds > 8 {
                            self.self_rescue_rounds = 0;
                            return Ok(AgentOutput { content, confidence: ConfidenceLevel::Low, verification_report: Some(verified.report), summary: Some("Self-rescue exhausted".into()) });
                        }
                        self.phase = HarnessPhase::Generator;
                        let prompt = self.trigger_fresh_rescue(self.self_rescue_rounds, &verified.details);
                        self.conversation.add_message(Message::System(
                            crate::types::message::SystemMessage { content: prompt }
                        ));
                        continue;
                    }
                    self.self_rescue_rounds = 0;
                    return Ok(AgentOutput { content, confidence: verified.confidence, verification_report: Some(verified.report), summary: None });
                }
                TaskType::Question | TaskType::Conversation => {
                    return Ok(AgentOutput { content, confidence: ConfidenceLevel::Medium, verification_report: None, summary: None });
                }
            }
        }
        self.exit_with_summary().await
    }
}
