use crate::agent::confidence::ConfidenceScorer;
use crate::agent::context::{ContextManager, FoldAction};
use crate::agent::conversation::ConversationState;
use crate::agent::harness::SprintContract;
use crate::agent::output::{AgentOutput, ConfidenceLevel, VerificationResult};
use crate::agent::system_prompt::{HarnessPhase, SystemPromptBuilder};
use crate::error::{AgentError, AgentResult};
use crate::llm::client::{LlmClient, LlmRequest, LlmResponse};
use crate::llm::scorer::CodeScorer;
use crate::tool_system::registry::ToolRegistry;
use crate::tool_system::repair::ToolCallRepair;
use crate::types::config::AgentConfig;
use crate::types::message::{
    Message, SystemMessage, ToolResultMessage, ToolUse, UserMessage,
};
use crate::types::tool::{ExecutionMode, TaskType, ToolContext};
use std::sync::Arc;

/// In-memory task item synced with todo_write tool
#[derive(Debug, Clone)]
pub struct TodoItem {
    pub subject: String,
    pub description: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus { Pending, InProgress, Completed }

/// Agent 主循环。ReAct 模式: 思考→决策→执行→验证→回复。
///
/// 状态机: User Input → [Fold?] → LLM Call → [Tool?] → [Verify?] → Output
///
/// 核心创新嵌入:
/// - 三体分离 (Planner→Generator→Evaluator) — 同一模型, 不同 prompt, 独立上下文
/// - SprintContract 验收契约 — 写代码前先制定验收标准
/// - 信心评分 — 推理链结构特征 6 权重扣分制
/// - 工具修复 4-pass — Scavenge/Truncation/Storm/Flatten
/// - Cache-First 上下文管理 — 缓存断点 + 6级自适应折叠
pub struct AgentLoop<L: LlmClient> {
    /// 全局配置
    config: AgentConfig,
    /// LLM 客户端 (trait, 可 Mock)
    llm: Arc<L>,
    /// 工具注册中心
    registry: Arc<ToolRegistry>,
    /// 系统提示构建器
    system_prompt: Arc<SystemPromptBuilder>,
    /// 上下文管理器 (折叠/缓存)
    context_mgr: ContextManager,
    /// 对话状态
    conversation: ConversationState,
    /// 当前任务的 SprintContract (Planner 在任务开始前创建)
    active_contract: Option<SprintContract>,
    /// 本轮是否已折叠 (防止单轮多次折叠)
    already_folded_this_turn: bool,
    /// 当前执行模式
    mode: ExecutionMode,
    /// 信心评分器 (基于 CoT 结构特征)
    confidence_scorer: ConfidenceScorer,
    /// 工具调用修复管线 (4-pass: Scavenge/Truncation/Storm/Flatten)
    repair: ToolCallRepair,
    /// 代码评分器 (trait in core, impl from external or RuleBasedScorer)
    code_scorer: Option<Arc<dyn CodeScorer>>,
    /// 记忆检索回调 (external crate 注入, core 不依赖 memory crate)
    memory_retrieve: Option<Arc<dyn Fn(&str) -> String + Send + Sync>>,
    /// 代码图谱查询回调 (external crate 注入, core 不依赖 code-graph crate)
    graph_context: Option<Arc<dyn Fn(&str) -> String + Send + Sync>>,
    /// 用户询问回调 — agent 主动暂停请求用户决策 (CLI/UI 注入)
    ask_user: Option<Arc<dyn Fn(&str, &str) -> String + Send + Sync>>,
    /// 沙箱实例 (sandbox crate 注入, core 不依赖 sandbox)
    sandbox: Option<Arc<std::sync::Mutex<Box<dyn crate::types::sandbox::SandboxInstance>>>>,
    /// 共享配置 (CLI 注入, 允许运行时 /slash 修改)
    shared_config: Option<Arc<std::sync::RwLock<AgentConfig>>>,
    /// Skill 系统提示注入文本
    skill_injection: Option<String>,
    /// 代码库概览 (codebase overview, 启动时生成)
    codebase_overview: Option<String>,
    /// 当前轮的任务追踪列表 (从 todo_write 工具同步，预留未来UI展示)
    #[allow(dead_code)]
    active_todos: Vec<TodoItem>,
    /// 三体阶段 (Planner → Generator → Evaluator)
    phase: HarnessPhase,
    /// Pain6 自救计数器 — 同一任务连续验证失败的次数。
    /// 超过 max_self_rescue_rounds 后触发 ask_user 降级。
    self_rescue_rounds: u32,
    /// Planner 阶段已执行轮数 (用于自动推进)
    planner_turns: u32,
    /// Tool progress streaming callback (set per-turn by run_streaming)
    tool_progress_tx: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

impl<L: LlmClient> AgentLoop<L> {
    /// 创建新的 Agent 循环。
    pub fn new(
        config: AgentConfig,
        llm: Arc<L>,
        registry: Arc<ToolRegistry>,
        system_prompt: Arc<SystemPromptBuilder>,
    ) -> Self {
        let context_mgr =
            ContextManager::new(config.clone(), 4); // 缓存断点在倒数第4条消息

        Self {
            config,
            llm,
            registry,
            system_prompt,
            context_mgr,
            conversation: ConversationState::new(),
            active_contract: None,
            already_folded_this_turn: false,
            mode: ExecutionMode::Default,
            confidence_scorer: ConfidenceScorer::new(),
            repair: ToolCallRepair::new(),
            code_scorer: None,
            memory_retrieve: None,
            graph_context: None,
            ask_user: None,
            sandbox: None,
            shared_config: None,
            skill_injection: None,
            codebase_overview: None,
            active_todos: Vec::new(),
            phase: HarnessPhase::Generator,
            self_rescue_rounds: 0,
            planner_turns: 0,
            tool_progress_tx: None,
        }
    }

    /// Three-body phase auto-advancement.
    /// Planner → Generator after 2 turns (enough to survey).
    /// Generator → Evaluator when LLM produces final text output (no more tools).
    fn advance_phase(&mut self, turn: u32) {
        match self.phase {
            HarnessPhase::Planner => {
                self.planner_turns += 1;
                if self.planner_turns >= 2 {
                    self.phase = HarnessPhase::Generator;
                    tracing::info!("Three-body: Planner → Generator (turn {turn})");
                }
            }
            HarnessPhase::Generator => {
                // Transition handled after LLM response (when no tool calls)
            }
            HarnessPhase::Evaluator => {
                // Transition handled in verify_output (pass → done, fail → Generator)
            }
        }
    }

    /// Set three-body phase (called when mode changes)
    pub fn set_phase(&mut self, phase: HarnessPhase) { self.phase = phase; }
    pub fn phase(&self) -> HarnessPhase { self.phase }

    /// After tool execution, check if a plan tool was called → auto-create SprintContract.
    /// Also handles phase transitions: Planner→Generator after plan, Generator→Evaluator on output.
    fn try_auto_contract(&mut self, tool_uses: &[ToolUse]) {
        for tu in tool_uses {
            if tu.name == "plan" {
                if let Some(contract) = SprintContract::from_plan_json(&tu.input) {
                    tracing::info!("SprintContract auto-created from plan tool: {}", contract.objective);
                    self.active_contract = Some(contract);
                    // Plan created → switch from Planner to Generator
                    if self.phase == HarnessPhase::Planner {
                        self.phase = HarnessPhase::Generator;
                    }
                }
            }
            if tu.name == "todo_write" {
                if let Some(ref mut contract) = self.active_contract {
                    if let Some(tasks) = tu.input.get("tasks").and_then(|v| v.as_array()) {
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
        }
    }

    /// 注入代码评分器 (RuleBasedScorer 或外部实现)
    pub fn with_code_scorer(mut self, scorer: Arc<dyn CodeScorer>) -> Self {
        self.code_scorer = Some(scorer); self
    }

    /// 注入记忆检索 (memory crate 调用方注入闭包, core 无依赖)
    pub fn with_memory(mut self, retrieve: Arc<dyn Fn(&str) -> String + Send + Sync>) -> Self {
        self.memory_retrieve = Some(retrieve); self
    }

    /// 注入代码图谱查询 (code-graph crate 调用方注入闭包)
    pub fn with_graph(mut self, query: Arc<dyn Fn(&str) -> String + Send + Sync>) -> Self {
        self.graph_context = Some(query); self
    }

    /// 注入 Skill 系统提示 (初始化时)
    pub fn with_skills(mut self, injection: String) -> Self {
        self.skill_injection = Some(injection); self
    }

    /// 运行时追加 Skill 提示 (e.g. 用户调用 /skill name)
    pub fn append_skill_injection(&mut self, injection: &str) {
        if let Some(ref mut existing) = self.skill_injection {
            existing.push_str("\n\n");
            existing.push_str(injection);
        } else {
            self.skill_injection = Some(injection.to_string());
        }
    }

    /// 注入代码库概览 (codebase overview, 启动时生成一次)
    pub fn with_codebase_overview(mut self, overview: String) -> Self {
        self.codebase_overview = Some(overview); self
    }

    /// Set codebase overview at runtime (called from background task after startup).
    pub fn set_codebase_overview(&mut self, overview: String) {
        if !overview.is_empty() { self.codebase_overview = Some(overview); }
    }

    /// 注入用户响应回调 (CLI/UI 注入, agent 调用 ask_user 工具时触发)
    /// callback 接收 (question_json, header) → 返回 user's answer
    pub fn with_ask_user(mut self, cb: Arc<dyn Fn(&str, &str) -> String + Send + Sync>) -> Self {
        self.ask_user = Some(cb); self
    }

    /// 注入沙箱实例 (sandbox crate 注入, core 不依赖 sandbox)
    pub fn with_sandbox(mut self, instance: Arc<std::sync::Mutex<Box<dyn crate::types::sandbox::SandboxInstance>>>) -> Self {
        self.sandbox = Some(instance); self
    }

    /// Check if the active goal (SprintContract) is met using a cheap Flash call.
    /// Returns Some("YES") if goal is met, Some("NO") if not, None on error.
    async fn check_goal_completed(&self, latest_output: &str) -> Option<String> {
        let contract = self.active_contract.as_ref()?;
        if contract.acceptance_criteria.is_empty() { return None; }
        let criteria: Vec<String> = contract.acceptance_criteria.iter()
            .map(|c| c.description.clone()).collect();
        let criteria_str = criteria.join("; ");
        let prompt = format!(
            "Goal: {}\nSuccess criteria: {}\nLatest agent output: {}\n\nQuestion: Has the goal been fully achieved? Answer only YES or NO.",
            contract.objective, criteria_str,
            &latest_output[..latest_output.len().min(3000)]
        );
        let config = crate::llm::client::LlmRequest {
            model: "deepseek-v4-flash".into(),
            max_tokens: 10,
            temperature: 0.0,
            reasoning_effort: crate::types::tool::ReasoningEffort::Off,
            timeout: std::time::Duration::from_secs(30),
            user_id: self.config.user_id.clone(),
            thinking_enabled: false,
            strict_schema: false,
            web_search_enabled: false,
            tools_json: String::new(),
        };
        let messages = vec![crate::types::message::Message::User(
            crate::types::message::UserMessage {
                id: "goal_check".into(),
                timestamp: chrono::Utc::now(),
                content: prompt,
                metadata: Default::default(),
            }
        )];
        match self.llm.chat("You are a goal verification assistant. Answer only YES or NO.", &messages, &config).await {
            Ok(resp) => {
                let answer = resp.content.unwrap_or_default().trim().to_uppercase();
                if answer.contains("YES") { Some("YES".into()) }
                else { Some("NO".into()) }
            }
            Err(_) => None,
        }
    }

    /// 手动触发上下文压缩。保留最近 25% 的消息，其余折叠为摘要。
    pub fn compact_now(&mut self) -> String {
        let before = self.conversation.estimated_tokens();
        let eff_max = self.effective_ctx_max();
        let tail = (eff_max as f32 * 0.25) as usize;
        let result = self.context_mgr.execute_fold(
            self.conversation.messages_mut(),
            tail,
            &|head: &[Message]| format!("[Context compressed: {} earlier messages summarized]", head.len()),
        );
        let after = self.conversation.estimated_tokens();
        if result.folded {
            format!("Compacted: {} → {} tokens ({} messages → {})", before, after, result.before, result.after)
        } else {
            format!("No compaction needed ({} tokens, savings < 30%)", before)
        }
    }

    /// 注入共享配置 (CLI 注入, 允许运行时通过 slash commands 修改)
    pub fn with_shared_config(mut self, cfg: Arc<std::sync::RwLock<AgentConfig>>) -> Self {
        self.shared_config = Some(cfg); self
    }

    fn sync_config(&mut self) {
        if let Some(ref cfg) = self.shared_config {
            let c = cfg.read().unwrap_or_else(|e| e.into_inner());
            self.config.thinking_enabled = c.thinking_enabled;
            self.config.web_search_enabled = c.web_search_enabled;
            self.config.reasoning_effort = c.reasoning_effort.clone();
            self.config.verify_before_output = c.verify_before_output;
            self.config.auto_model_routing = c.auto_model_routing;
            self.config.snapshots_enabled = c.snapshots_enabled;
        }
    }


    /// 流式运行 — 每个 LLM 响应块通过 callback 推送。
    /// `on_stream` 在每块文本/token 到达时调用，TUI 可用其实时渲染。
    pub async fn run_streaming(
        &mut self,
        user_input: &str,
        on_stream: &(dyn Fn(crate::llm::client::StreamEvent) + Send + Sync),
    ) -> AgentResult<AgentOutput> {
        // 注入用户消息
        self.conversation.add_message(Message::User(UserMessage {
            id: format!("msg_{}", uuid::Uuid::new_v4()),
            timestamp: chrono::Utc::now(),
            content: user_input.to_string(),
            metadata: Default::default(),
        }));
        // 注入记忆作为 SystemMessage（不在 system prompt 中，保护缓存）
        self.inject_memory_message(user_input);
        self.already_folded_this_turn = false;
        self.self_rescue_rounds = 0;

        // Set up tool progress streaming for this turn
        let (prog_tx, prog_rx) = std::sync::mpsc::channel::<String>();
        self.tool_progress_tx = Some(std::sync::Arc::new(move |line: String| {
            let _ = prog_tx.send(line);
        }));

        // 预飞检查
        let preflight = self.context_mgr.preflight_check(self.conversation.estimated_tokens(), self.effective_ctx_max());
        if let Some(FoldAction::Fold { tail_budget, .. }) = preflight {
            self.context_mgr.execute_fold(self.conversation.messages_mut(), tail_budget,
                &|head: &[Message]| format!("Summarized {} earlier messages.", head.len()));
            self.already_folded_this_turn = true;
        }

        // ── 三体状态机初始化 ──
        // Plan模式: Planner→计划书; Default/Yolo: 跳过Planner直接Generator
        if self.mode == ExecutionMode::Plan {
            self.phase = HarnessPhase::Planner;
            self.planner_turns = 0;
        } else if self.phase != HarnessPhase::Evaluator {
            self.phase = HarnessPhase::Generator;
        }

        for turn in 0..self.config.max_turns {
            // ── 同步运行时配置 (来自 TUI slash commands) ──
            self.sync_config();

            // ── 三体: 阶段自动推进 ──
            self.advance_phase(turn);

            // Shrink tool results before fold (token-aware, CJK-safe)
            let est_before_shrink = self.conversation.estimated_tokens();
            let eff_max = self.effective_ctx_max();
            if est_before_shrink > eff_max * 3 / 4 {
                crate::agent::healing::shrink_tool_results_by_tokens(
                    self.conversation.messages_mut(),
                    eff_max / 20, // 5% per tool result
                );
            }

            // 折叠检查
            let fold_action = self.context_mgr.decide_fold_action(self.conversation.estimated_tokens(), eff_max, self.already_folded_this_turn);
            match fold_action {
                FoldAction::Fold { tail_budget, .. } => {
                    self.context_mgr.execute_fold(self.conversation.messages_mut(), tail_budget,
                        &|head: &[Message]| format!("Summarized {} earlier messages.", head.len()));
                    self.already_folded_this_turn = true;
                }
                FoldAction::ExitWithSummary { .. } => return self.exit_with_summary().await,
                FoldAction::None => {}
            }

            // Hard guard: don't call LLM if context is too full
            let est = self.conversation.estimated_tokens();
            let eff_max = self.effective_ctx_max();
            if est > eff_max * 9 / 10 {
                return self.exit_with_summary().await;
            }

            // 流式 LLM 调用
            let system = self.build_system_prompt(user_input);
            let messages = self.conversation.messages();
            let tools_json = self.registry.get_anthropic_tools_json();
            let request = LlmRequest {
                model: self.config.default_model.clone(),
                max_tokens: 393_216,
                temperature: 0.0,
                reasoning_effort: parse_reasoning_effort(&self.config.reasoning_effort),
                timeout: std::time::Duration::from_secs(120),
                user_id: self.config.user_id.clone(),
                thinking_enabled: self.config.thinking_enabled,
                strict_schema: self.config.strict_tool_schema,
                web_search_enabled: self.config.web_search_enabled,
                tools_json,
            };

            let llm_start = std::time::Instant::now();
            let _est_tokens = est;
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
                    return Err(crate::error::AgentError::Internal(
                        format!("LLM call timed out after {waited}s")
                    ));
                }
            };

            // 工具调用 → 修复 → 并行执行
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
                        return Err(crate::error::AgentError::Internal("Tool execution timed out".into()));
                    }
                };
                // Save assistant message WITH tool_uses AFTER tool execution
                // (so ToolResult is guaranteed to follow)
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
                // Emit tool results through streaming callback
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
                // Drain progress before saving tool results
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

            // ── 三体: Generator 产出文本 → 切换到 Evaluator ──
            if self.phase == HarnessPhase::Generator && !content.trim().is_empty() {
                self.phase = HarnessPhase::Evaluator;
            }

            // ── Goal check: if SprintContract has acceptance criteria, ask Flash if done ──
            if let Some(ref contract) = self.active_contract {
                if !contract.acceptance_criteria.is_empty() {
                    match self.check_goal_completed(&content).await {
                        Some(ref answer) if answer == "YES" => {
                            on_stream(crate::llm::client::StreamEvent::TextDelta(
                                "\n\n[PASS]  Goal achieved! All acceptance criteria met.".into()
                            ));
                            // Save and return
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
                        _ => {} // Continue loop
                    }
                }
            }

            // 保存 Assistant 消息
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
                        let rescue_prompt = match self.self_rescue_rounds {
                            1 => format!(
                                "## Verification Failed — Round 1\n\n\
                                Issues found:\n{}\n\n\
                                Fix each issue carefully:\n\
                                - Re-read the files you're modifying to confirm current state\n\
                                - Run the tests to reproduce failures before fixing\n\
                                - Check edge cases: empty input, null values, error paths\n\
                                - After fixing, run tests again to verify",
                                verified.details
                            ),
                            2 => format!(
                                "## Still Failing — Round 2\n\n\
                                Remaining issues:\n{}\n\n\
                                Take a step back:\n\
                                - Are you fixing the root cause or just symptoms?\n\
                                - Run `cargo test` and paste the FULL output\n\
                                - Check if existing tests actually cover your changes\n\
                                - Look at git diff to confirm only intended files changed",
                                verified.details
                            ),
                            3..=5 => format!(
                                "## Still Failing — Round {}\n\n\
                                {}\n\n\
                                Try a different approach:\n\
                                - Is there a simpler way to achieve the goal?\n\
                                - Compare with how existing code in the codebase handles similar cases\n\
                                - If you're stuck on a specific error, isolate it to a minimal reproduction",
                                self.self_rescue_rounds, verified.details
                            ),
                            _ => format!(
                                "## Final Attempt — Round {}\n\n\
                                {}\n\n\
                                This is your last chance. Consider:\n\
                                - Abandoning the current approach and starting fresh\n\
                                - Asking the user for clarification or help\n\
                                - Reporting what you tried and why it didn't work",
                                self.self_rescue_rounds, verified.details
                            ),
                        };
                        self.phase = HarnessPhase::Generator; // Evaluator fail → back to Generator
                        self.conversation.add_message(Message::System(SystemMessage { content: rescue_prompt }));
                        continue;
                    }
                    self.self_rescue_rounds = 0;
                    return Ok(AgentOutput { content, confidence: verified.confidence, verification_report: Some(verified.report), summary: None });
                }
                TaskType::Question | TaskType::Conversation => {
                    let raw = self.confidence_scorer.score(response.reasoning.as_deref().unwrap_or(""));
                    return Ok(AgentOutput { content, confidence: score_to_level(raw), verification_report: None, summary: None });
                }
            }
        }
        self.exit_with_summary().await
    }

    /// 运行一次完整的 Agent 任务。
    /// 从用户输入开始，到返回最终回复或达到 max_turns 上限。
    pub async fn run(&mut self, user_input: &str) -> AgentResult<AgentOutput> {
        let session_id = self.conversation.started_at().to_rfc3339();
        tracing::info!(session_id, user_input = %user_input, "agent.run.start");

        // 回合开始: 注入用户消息
        self.conversation.add_message(Message::User(UserMessage {
            id: format!("msg_{}", uuid::Uuid::new_v4()),
            timestamp: chrono::Utc::now(),
            content: user_input.to_string(),
            metadata: Default::default(),
        }));
        self.already_folded_this_turn = false;
        self.self_rescue_rounds = 0;

        // ── 三体: 任务复杂度评估 → 自动切换 Planner ──
        if self.phase == HarnessPhase::Generator && is_complex_task(user_input) {
            self.phase = HarnessPhase::Planner;
        }

        // 预飞检查: 回合开始前是否需要预折叠 (会话恢复/大粘贴)
        let preflight = self.context_mgr.preflight_check(
            self.conversation.estimated_tokens(),
            self.effective_ctx_max(),
        );
        if let Some(FoldAction::Fold {
            tail_budget, ..
        }) = preflight
        {
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

        // 主循环: 每轮执行 LLM 调用 → 工具 → 验证
        for turn in 0..self.config.max_turns {
            // ── 同步运行时配置 + 三体阶段推进 ──
            self.sync_config();
            self.advance_phase(turn);

            // (a) 检查上下文折叠
            let fold_action = self.context_mgr.decide_fold_action(
                self.conversation.estimated_tokens(),
                self.effective_ctx_max(),
                self.already_folded_this_turn,
            );

            match fold_action {
                FoldAction::Fold {
                    tail_budget, ..
                } => {
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
                FoldAction::ExitWithSummary { .. } => {
                    return self.exit_with_summary().await;
                }
                FoldAction::None => { /* 继续 */ }
            }

            // (b) 调用 LLM
            let response = match self.call_llm_with_recovery(user_input).await {
                Ok(r) => r,
                Err(e) => {
                    // Pass@3 恢复失败 → 返回错误
                    return Err(e);
                }
            };

            // (c) 工具调用 → 修复 → 并行执行 → 注入结果
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
                // Save assistant message WITH tool_uses BEFORE tool results
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

            // (d) 纯文本回复 → 分类任务类型 → 决定验证策略
            let content = response.content.unwrap_or_default();

            // ── 三体: Generator 产出文本 → Evaluator ──
            if self.phase == HarnessPhase::Generator && !content.trim().is_empty() {
                self.phase = HarnessPhase::Evaluator;
            }

            // 保存 Assistant 回复到对话历史 (多轮对话正确性)
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

            // 记录 token 使用和成本
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

                    // ── Pain6 硬门控: 低信心/阻塞问题 → 自救循环 ──
                    if !verified.passed {
                        self.self_rescue_rounds += 1;

                        if self.self_rescue_rounds > 8 {
                            // 自救耗尽 → 返回当前最佳结果，标记低信心
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

                        // Self-rescue prompt — escalating detail per round
                        let rescue_prompt = match self.self_rescue_rounds {
                            1 => format!(
                                "## Verification Failed — Round 1\n\n\
                                Issues found:\n{}\n\n\
                                Fix each issue carefully:\n\
                                - Re-read the files you're modifying to confirm current state\n\
                                - Run the tests to reproduce failures before fixing\n\
                                - Check edge cases: empty input, null values, error paths\n\
                                - After fixing, run tests again to verify",
                                verified.details
                            ),
                            2 => format!(
                                "## Still Failing — Round 2\n\n\
                                Remaining issues:\n{}\n\n\
                                Take a step back:\n\
                                - Are you fixing the root cause or just symptoms?\n\
                                - Run `cargo test` and paste the FULL output\n\
                                - Check if existing tests actually cover your changes\n\
                                - Look at git diff to confirm only intended files changed",
                                verified.details
                            ),
                            3..=5 => format!(
                                "## Still Failing — Round {}\n\n\
                                {}\n\n\
                                Try a different approach:\n\
                                - Is there a simpler way to achieve the goal?\n\
                                - Compare with how existing code handles similar cases\n\
                                - If stuck on a specific error, isolate it to a minimal reproduction",
                                self.self_rescue_rounds, verified.details
                            ),
                            _ => format!(
                                "## Final Attempt — Round {}\n\n\
                                {}\n\n\
                                Last chance.\n\
                                - Consider abandoning current approach and starting fresh\n\
                                - Ask the user for clarification if needed\n\
                                - Report what you tried and why it didn't work",
                                self.self_rescue_rounds, verified.details
                            ),
                        };

                        self.conversation
                            .add_message(Message::System(SystemMessage {
                                content: rescue_prompt,
                            }));
                        self.phase = HarnessPhase::Generator; // Evaluator fail → back to Generator
                        continue;
                    }

                    // 通过 → 重置计数器
                    self.self_rescue_rounds = 0;
                    return Ok(AgentOutput {
                        content,
                        confidence: verified.confidence,
                        verification_report: Some(verified.report),
                        summary: None,
                    });
                }
                TaskType::Question | TaskType::Conversation => {
                    // 非代码任务: ConfidenceScorer 评估 (基于 CoT 结构特征)
                    let raw_score = self
                        .confidence_scorer
                        .score(response.reasoning.as_deref().unwrap_or(""));
                    let confidence = score_to_level(raw_score);
                    return Ok(AgentOutput {
                        content,
                        confidence,
                        verification_report: None,
                        summary: None,
                    });
                }
            }
        }

        // max_turns 耗尽
        self.exit_with_summary().await
    }

    // ── 内部方法 ──

    /// 调用 LLM (带 Pass@3 错误恢复)。
    /// 非可重试错误 (400/401/402/403/404/405/422) 不重试。
    async fn call_llm_with_recovery(&mut self, user_input: &str) -> AgentResult<LlmResponse> {
        let mut last_err = None;
        let mut budget_ratio = 1.0f32;

        let system = self.build_system_prompt(user_input);
        let messages = self.conversation.messages();

        for attempt in 0..self.config.retry_max_attempts {
            let tools_json = self.registry.get_anthropic_tools_json();
            let request = LlmRequest {
                model: self.config.default_model.clone(),
                max_tokens: (393_216.0 * budget_ratio) as u32,
                temperature: 0.0,
                reasoning_effort: parse_reasoning_effort(&self.config.reasoning_effort),
                timeout: std::time::Duration::from_secs(120),
                user_id: self.config.user_id.clone(),
                thinking_enabled: self.config.thinking_enabled,
                strict_schema: self.config.strict_tool_schema,
                web_search_enabled: self.config.web_search_enabled,
                tools_json,
            };

            match self.llm.chat(&system, messages, &request).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if !is_retryable(&e) {
                        return Err(e);
                    }
                    last_err = Some(e);
                    // 单调递减 budget: 100% → 60% → 30%
                    budget_ratio *= 0.6;
                    if (attempt as usize) < self.config.retry_max_attempts as usize - 1 {
                        let delay_ms =
                            self.config.retry_initial_backoff_ms * 2u64.pow(attempt);
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| AgentError::Internal("Max retries exceeded".into())))
    }

    /// 并行执行工具调用。
    async fn execute_tools_parallel(
        &self,
        tool_uses: &[ToolUse],
    ) -> AgentResult<Vec<ToolResultMessage>> {
        if tool_uses.is_empty() {
            return Ok(vec![]);
        }

        // Tool progress streaming: emit lines as ToolProgress events
        let _tool_id = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let progress_tx: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>> =
            self.tool_progress_tx.as_ref().map(|tx| tx.clone());

        let ctx = ToolContext {
            working_dir: self.config.workspace_dir.clone().into(),
            permission_mode: crate::types::tool::PermissionMode::Default,
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

        // 分批并行执行: 每批最多 max_parallel 个工具
        for chunk in tool_uses.chunks(max_parallel) {
            let mut handles = Vec::with_capacity(chunk.len());
            for tu in chunk {
                // Intercept ask_user: call the UI callback instead of executing the tool
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
                            content: vec![crate::types::message::ContentBlock::Text { text: answer }],
                            elapsed_ms: 0,
                        });
                        continue;
                    } else {
                        // Callback not set — log visible error
                        results.push(ToolResultMessage {
                            tool_use_id: tu.id.clone(),
                            is_error: true,
                            content: vec![crate::types::message::ContentBlock::Text {
                                text: "BUG: ask_user callback is NOT set in AgentLoop. TUI must call with_ask_user().".into()
                            }],
                            elapsed_ms: 0,
                        });
                        continue;
                    }
                }

                let is_safe = self.registry.concurrency_safety_of(&tu.name) == crate::types::tool::ConcurrencySafety::ConcurrentSafe;
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
                    // ConcurrentUnsafe tools (e.g. code graph) run synchronously
                    let result = self.registry.execute(&tu, &ctx).await;
                    match result {
                        Ok(r) => results.push(r),
                        Err(e) => results.push(ToolResultMessage {
                            tool_use_id: tu.id.clone(), is_error: true,
                            content: vec![crate::types::message::ContentBlock::Text {
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
                        content: vec![crate::types::message::ContentBlock::Text {
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
                        content: vec![crate::types::message::ContentBlock::Text {
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

    /// 验证代码输出。Multi-phase: heuristic checks + compiler diagnostics + test execution + git review.
    async fn verify_output(&mut self, content: &str) -> AgentResult<VerificationResult> {
        let mut blocking = 0u32;
        let mut advisory = 0u32;
        let mut details: Vec<String> = Vec::new();
        let workspace = std::path::PathBuf::from(&self.config.workspace_dir);

        // ── Phase 1: Heuristic checks ──
        // Empty output
        if content.trim().is_empty() {
            blocking += 1;
            details.push("Empty output — no code or text produced".into());
        }

        // CodeScorer scoring
        if let Some(ref scorer) = self.code_scorer {
            let score = scorer.score("", content, "");
            if score < 0.4 {
                blocking += 1;
                details.push(format!("Quality score too low: {:.2} (threshold: 0.4)", score));
            } else if score < 0.7 {
                advisory += 1;
                details.push(format!("Quality score marginal: {:.2} (threshold: 0.7)", score));
            }
        }

        // Incomplete work markers
        let todo_fixme_count = content.matches("TODO").count() + content.matches("FIXME").count();
        if todo_fixme_count > 0 {
            advisory += 1;
            details.push(format!("{} TODO/FIXME markers found", todo_fixme_count));
        }

        // Unsafe blocks in Rust
        if content.contains("unsafe ") && !content.contains("unsafe {") {
            advisory += 1;
            details.push("Contains 'unsafe' keyword — verify safety invariants".into());
        }

        // unwrap() calls in new code (fragile error handling)
        let unwrap_count = content.matches(".unwrap()").count();
        if unwrap_count > 3 {
            advisory += 1;
            details.push(format!("{} .unwrap() calls — consider proper error handling", unwrap_count));
        }

        // ── Phase 2: Compiler diagnostics ──
        let diag_output = std::process::Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(&workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        if let Ok(output) = diag_output {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let error_lines: Vec<&str> = stderr.lines()
                .filter(|l| l.contains("error") || l.contains("error:"))
                .take(10)
                .collect();
            if !error_lines.is_empty() {
                blocking += error_lines.len() as u32;
                for e in &error_lines[..error_lines.len().min(5)] {
                    details.push(format!("Compiler error: {}", e));
                }
                if error_lines.len() >= 5 {
                    details.push(format!("... and {} more errors", error_lines.len() - 5));
                }
            }

            let warn_lines: Vec<&str> = stderr.lines()
                .filter(|l| l.contains("warning") || l.contains("warning:"))
                .take(5)
                .collect();
            if !warn_lines.is_empty() {
                advisory += warn_lines.len() as u32;
                for w in &warn_lines[..warn_lines.len().min(3)] {
                    details.push(format!("Warning: {}", w));
                }
            }

            if !output.status.success() && error_lines.is_empty() {
                // cargo check failed but no parseable errors — report raw
                let truncated: String = stderr.lines().take(5).collect::<Vec<_>>().join("\n");
                blocking += 1;
                details.push(format!("cargo check FAILED:\n{}", truncated));
            }
        }

        // ── Phase 3: Test execution ──
        let test_output = std::process::Command::new("cargo")
            .args(["test", "--lib", "--no-fail-fast"])
            .current_dir(&workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        if let Ok(output) = test_output {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Parse test results
            let mut passed = 0u32;
            let mut failed = 0u32;
            for line in stderr.lines().chain(stdout.lines()) {
                if line.contains("test result:") {
                    for part in line.split(';') {
                        let part = part.trim();
                        if let Some(num) = part.split_whitespace().next().and_then(|n| n.parse::<u32>().ok()) {
                            if part.contains("passed") { passed = num; }
                            else if part.contains("failed") { failed = num; }
                        }
                    }
                }
            }

            if !output.status.success() || failed > 0 {
                blocking += failed;
                details.push(format!("Tests: {} passed, {} FAILED", passed, failed));

                // Extract specific failures for rescue prompt
                let failures: Vec<&str> = stderr.lines()
                    .filter(|l| l.contains("FAILED") || l.contains("panicked") || l.contains("assertion"))
                    .take(8)
                    .collect();
                for f in &failures[..failures.len().min(5)] {
                    details.push(format!("Test failure: {}", f));
                }
            } else if passed > 0 {
                details.push(format!("Tests: {} passed, 0 failed", passed));
            }
        }

        // ── Phase 4: Git change review ──
        let git_diff = std::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        if let Ok(output) = git_diff {
            let stat = String::from_utf8_lossy(&output.stdout);
            if !stat.trim().is_empty() {
                let changed_files: Vec<&str> = stat.lines()
                    .filter(|l| l.contains('|'))
                    .take(10)
                    .collect();
                let file_count = changed_files.len();
                details.push(format!("Files changed: {}", file_count));
                // Warn if many files changed (may indicate unintended modifications)
                if file_count > 5 {
                    advisory += 1;
                    details.push("More than 5 files modified — verify all changes are intended".into());
                }
                // Check for changes to sensitive files
                for cf in &changed_files {
                    let path = cf.split('|').next().unwrap_or("").trim();
                    if path.contains("Cargo.lock") || path.contains(".gitignore") {
                        advisory += 1;
                        details.push(format!("Sensitive file changed: {}", path));
                    }
                }
            }
        }

        // ── Phase 5: SprintContract check ──
        if let Some(ref contract) = self.active_contract {
            if !contract.is_complete() {
                let (done, total) = contract.progress();
                blocking += 1;
                details.push(format!("Contract: {}/{} tasks incomplete", total - done, total));
            }

            let blocked = contract.blocked_tasks();
            if !blocked.is_empty() {
                advisory += 1;
                details.push(format!("{} tasks blocked by dependencies", blocked.len()));
            }

            for criterion in &contract.acceptance_criteria {
                if let Some(ref expected) = criterion.expected_output_contains {
                    if !content.contains(expected) {
                        advisory += 1;
                        details.push(format!(
                            "Acceptance '{}': expected '{}' not found in output",
                            criterion.description, expected
                        ));
                    }
                }
            }

            self.phase = HarnessPhase::Evaluator;
        }

        // ── Verdict ──
        if blocking > 0 {
            Ok(VerificationResult::failed(
                blocking,
                advisory,
                details.join("\n"),
                format!("FAIL: {} blocking, {} advisory issues", blocking, advisory),
            ))
        } else if advisory > 0 {
            Ok(VerificationResult::passed(format!(
                "PASS with {} advisory issues:\n{}",
                advisory,
                details.join("\n")
            )))
        } else {
            Ok(VerificationResult::passed(
                "All checks passed — no issues found.".into()
            ))
        }
    }

    /// 上下文溢出时退出并总结。先尝试 force summary (force-summary pattern)，
    /// 给 LLM 最后一次机会用极短回复总结当前状态，失败则返回静态提示。
    async fn exit_with_summary(&self) -> AgentResult<AgentOutput> {
        // Try one last minimal LLM call to get a summary (force-summary pattern)
        let force_prompt = crate::agent::healing::force_summary_prompt();
        let config = crate::llm::client::LlmRequest {
            model: "deepseek-v4-flash".into(),
            max_tokens: 300,
            temperature: 0.0,
            reasoning_effort: crate::types::tool::ReasoningEffort::Off,
            timeout: std::time::Duration::from_secs(15),
            user_id: self.config.user_id.clone(),
            thinking_enabled: false,
            strict_schema: false,
            web_search_enabled: false,
            tools_json: String::new(),
        };
        let messages = vec![Message::User(crate::types::message::UserMessage {
            id: "force_summary".into(),
            timestamp: chrono::Utc::now(),
            content: force_prompt,
            metadata: Default::default(),
        })];

        match self.llm.chat("Be brief.", &messages, &config).await {
            Ok(resp) => {
                let summary = resp.content.unwrap_or_default();
                Ok(AgentOutput {
                    content: format!("Context nearly full. Forced summary:\n\n{}", summary),
                    confidence: ConfidenceLevel::Low,
                    verification_report: None,
                    summary: Some("Context overflow — forced summary".into()),
                })
            }
            Err(_) => Ok(AgentOutput {
                content: "Context nearly full. Use /clear to free space or start a new session.".into(),
                confidence: ConfidenceLevel::Low,
                verification_report: None,
                summary: Some("Context overflow — task incomplete".into()),
            }),
        }
    }

    /// 构建当前系统提示 (含记忆注入 + 图谱上下文)
    fn build_system_prompt(&self, user_input: &str) -> String {
        let tool_json = self.registry.get_cached_tool_list_json();

        let memories = self.memory_retrieve.as_ref()
            .map(|cb| cb(user_input));

        let graph_ctx = self.graph_context.as_ref()
            .map(|cb| cb(user_input));

        let merged = {
            let overview = self.codebase_overview.clone();
            let memory = memories.map(|s| s.to_string());
            let graph = graph_ctx.map(|s| s.to_string());
            let skills = self.skill_injection.clone();
            let mut parts: Vec<String> = Vec::new();
            if let Some(ref o) = overview { if !o.is_empty() { parts.push(o.clone()); } }
            if let Some(ref m) = memory { if !m.is_empty() { parts.push(m.clone()); } }
            if let Some(ref g) = graph { if !g.is_empty() { parts.push(g.clone()); } }
            if let Some(ref s) = skills { if !s.is_empty() { parts.push(s.clone()); } }
            if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
        };

        let mut prompt = self.system_prompt
            .build(&tool_json, merged.as_deref(), None, self.mode, &self.config.default_model);
        let phase_prompt = self.system_prompt.build_phase(self.phase);
        prompt.push_str(&phase_prompt);
        prompt
    }

    /// 有效的上下文上限 (0 = 自动从模型获取)。
    fn effective_ctx_max(&self) -> usize {
        if self.config.max_context_tokens == 0 {
            self.llm.model_info().max_context_tokens * 4 / 5 // 80%
        } else {
            self.config.max_context_tokens
        }
    }

    // ── 访问器 ──

    pub fn conversation(&self) -> &ConversationState {
        &self.conversation
    }

    pub fn conversation_mut(&mut self) -> &mut ConversationState {
        &mut self.conversation
    }

    pub fn set_contract(&mut self, contract: SprintContract) {
        self.active_contract = Some(contract);
    }

    pub fn clear_contract(&mut self) {
        self.active_contract = None;
    }

    pub fn set_goal(&mut self, contract: SprintContract) {
        self.active_contract = Some(contract);
    }

    pub fn set_mode(&mut self, mode: ExecutionMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> ExecutionMode {
        self.mode
    }

    /// Set the skill injection text directly (used by sub-agents for system prompt override).
    pub fn set_skills_injection(&mut self, text: String) {
        self.skill_injection = Some(text);
    }

    /// Inject memory context as a SystemMessage (not in system prompt — protects cache).
    fn inject_memory_message(&mut self, user_input: &str) {
        let memory = self.memory_retrieve.as_ref()
            .and_then(|cb| {
                let result = cb(user_input);
                if result.is_empty() { None } else { Some(result) }
            });
        if let Some(mem) = memory {
            self.conversation.add_message(Message::System(SystemMessage {
                content: format!("## Relevant Experience\n{}", mem),
            }));
        }
    }

    /// Inject a mid-turn steer message from the user (MidTurnSteer pattern).
    /// The message is added as a SystemMessage so the LLM sees it in the next turn iteration.
    pub fn steer(&mut self, content: &str) {
        const STEER_WRAPPER: &str = "[Mid-turn steer queued by the user. Do not treat this as a new task; \
            use it only as additional guidance for the current task after completing the current step.]";
        let wrapped = format!("{}\n\n{}", STEER_WRAPPER, content);
        self.conversation.add_message(Message::System(SystemMessage { content: wrapped }));
    }
}

// ── 自由函数 ──

/// 三体系统: 判断任务是否复杂，决定是否进入 Planner 阶段。
/// 复杂任务标准: 3+ 步骤 / 2+ 文件 / 新功能 / 重构 / 跨模块修改。
fn is_complex_task(user_input: &str) -> bool {
    let lower = user_input.to_lowercase();
    let complex_keywords = [
        "implement", "create", "build", "refactor", "migrate",
        "design", "architect", "restructure", "rewrite", "scaffold",
        "实现", "创建", "构建", "重构", "迁移", "设计", "架构",
        "module", "feature", "system", "pipeline",
    ];
    // Check for multi-file indicators
    let file_count = lower.matches("file").count()
        + lower.matches("文件").count()
        + lower.matches("crate").count()
        + lower.matches("module").count()
        + lower.matches("模块").count();
    let step_count = lower.matches(',').count()
        + lower.matches("step").count()
        + lower.matches("步骤").count()
        + lower.matches("first").count()
        + lower.matches("then").count()
        + lower.matches("首先").count()
        + lower.matches("然后").count();

    let has_complex_keyword = complex_keywords.iter().any(|k| lower.contains(k));
    has_complex_keyword || file_count >= 2 || step_count >= 2
}

/// 根据用户输入和 LLM 输出分类任务类型。
fn classify_task_type(user_input: &str, output: &str) -> TaskType {
    let combined = format!("{} {}", user_input, output);
    let lower = combined.to_lowercase();

    // 代码生成关键词 (长度≥5, 子串匹配安全)
    let gen_keywords = ["implement", "create", "build", "generate", "write a", "scaffold"];
    // 代码编辑关键词
    let edit_keywords = ["fix", "refactor", "change", "update", "modify", "edit", "edited", "editing", "remove"];
    // 问题关键词
    let question_keywords = ["what", "how", "why", "explain"];

    if question_keywords
        .iter()
        .any(|k| lower.starts_with(k) || lower.contains(&format!(" {}", k)))
        || lower.contains('?')
    {
        return TaskType::Question;
    }

    if gen_keywords.iter().any(|k| lower.contains(k)) {
        return TaskType::CodeGeneration;
    }

    // 编辑关键词: 词边界匹配，防止 "edit" 匹配 "credits"/"expedite"
    if edit_keywords
        .iter()
        .any(|k| is_word_match(k, &lower))
    {
        return TaskType::CodeEdit;
    }

    // 输出包含代码块 → 代码编辑
    if output.contains("```") {
        return TaskType::CodeEdit;
    }

    TaskType::Conversation
}

/// 检查 keyword 是否作为独立词出现在 text 中。
/// 词边界: keyword 前必须是非字母数字或开头, 后必须是非字母数字或结尾。
fn is_word_match(keyword: &str, text: &str) -> bool {
    if let Some(pos) = text.find(keyword) {
        let before = pos == 0 || {
            let c = text.as_bytes()[pos - 1];
            !c.is_ascii_alphanumeric()
        };
        let after = pos + keyword.len() >= text.len() || {
            let c = text.as_bytes()[pos + keyword.len()];
            !c.is_ascii_alphanumeric()
        };
        before && after
    } else {
        false
    }
}

/// 将 ConfidenceScorer 的原始 0.0-1.0 分数映射到 ConfidenceLevel。
fn score_to_level(raw_score: f32) -> ConfidenceLevel {
    if raw_score >= 0.9 {
        ConfidenceLevel::High
    } else if raw_score >= 0.6 {
        ConfidenceLevel::Medium
    } else {
        ConfidenceLevel::Low
    }
}

/// 判断 LLM 错误是否可重试。
/// 4xx 请求错误不重试 (客户端错误), 5xx/网络错误可重试。
fn is_retryable(err: &AgentError) -> bool {
    match err {
        AgentError::ApiUnreachable { .. } => true,
        AgentError::RateLimited { .. } => true,
        AgentError::ApiError { status, .. } => *status >= 500,
        _ => false,
    }
}

fn parse_reasoning_effort(s: &str) -> crate::types::tool::ReasoningEffort {
    match s.to_lowercase().as_str() {
        "off" => crate::types::tool::ReasoningEffort::Off,
        "high" => crate::types::tool::ReasoningEffort::High,
        "max" => crate::types::tool::ReasoningEffort::Max,
        _ => crate::types::tool::ReasoningEffort::Max,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// Mock LLM 客户端: 返回预设响应。
    struct MockLlm {
        response: LlmResponse,
        info: crate::llm::client::ModelInfo,
    }

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn chat(
            &self,
            _system_prompt: &str,
            _messages: &[Message],
            _config: &LlmRequest,
        ) -> AgentResult<LlmResponse> {
            Ok(self.response.clone())
        }

        fn model_info(&self) -> &crate::llm::client::ModelInfo {
            &self.info
        }
    }

    fn test_config() -> AgentConfig {
        let mut cfg = AgentConfig::default();
        cfg.verify_before_output = false; // 简化测试
        cfg.max_turns = 3; // 降低 max_turns 加速测试
        cfg
    }

    fn mock_llm(content: &str) -> Arc<MockLlm> {
        Arc::new(MockLlm {
            response: LlmResponse {
                content: Some(content.to_string()),
                reasoning: None,
                tool_uses: vec![],
                stop_reason: Some("end_turn".into()),
                usage: Default::default(),
                model: "mock".into(),
                latency_ms: 1,
            },
            info: crate::llm::client::ModelInfo {
                id: "mock".into(),
                provider: "mock".into(),
                max_context_tokens: 128_000,
                max_output_tokens: 8192,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
                cache_price_per_mtok: 0.0,
                supports_reasoning: false,
                supports_caching: false,
            },
        })
    }

    #[tokio::test]
    async fn test_simple_question_returns_answer() {
        let llm = mock_llm("Paris is the capital of France.");
        let registry = Arc::new(ToolRegistry::new());
        let sp = Arc::new(SystemPromptBuilder::new(test_config()));

        let mut agent = AgentLoop::new(test_config(), llm, registry, sp);
        let output = agent.run("What is the capital of France?").await.unwrap();

        assert!(output.content.contains("Paris"));
    }

    #[tokio::test]
    async fn test_code_task_goes_through_verification() {
        let config = {
            let mut c = AgentConfig::default();
            c.verify_before_output = true;
            c.max_turns = 3;
            c
        };

        let llm = Arc::new(MockLlm {
            response: LlmResponse {
                content: Some("fn main() { println!(\"hello\"); }".into()),
                reasoning: Some("This is a simple Rust program".into()),
                tool_uses: vec![],
                stop_reason: Some("end_turn".into()),
                usage: Default::default(),
                model: "mock".into(),
                latency_ms: 1,
            },
            info: crate::llm::client::ModelInfo {
                id: "mock".into(),
                provider: "mock".into(),
                max_context_tokens: 128_000,
                max_output_tokens: 8192,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
                cache_price_per_mtok: 0.0,
                supports_reasoning: false,
                supports_caching: false,
            },
        });

        let registry = Arc::new(ToolRegistry::new());
        let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

        let mut agent = AgentLoop::new(config, llm, registry, sp);
        // "implement" 触发 CodeGeneration 分类
        let output = agent.run("Implement a hello world program").await.unwrap();

        assert!(output.content.contains("fn main"));
        // 验证应该通过 (简单输出无 blocking issues)
        assert_eq!(output.confidence, ConfidenceLevel::High);
    }

    #[tokio::test]
    async fn test_tool_execution_loop() {
        let config = {
            let mut c = AgentConfig::default();
            c.verify_before_output = false;
            c.max_turns = 5;
            c
        };

        let llm = Arc::new(MockLlm {
            response: LlmResponse {
                content: None,
                reasoning: None,
                tool_uses: vec![],
                stop_reason: None,
                usage: Default::default(),
                model: "mock".into(),
                latency_ms: 0,
            },
            info: crate::llm::client::ModelInfo {
                id: "mock".into(),
                provider: "mock".into(),
                max_context_tokens: 128_000,
                max_output_tokens: 8192,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
                cache_price_per_mtok: 0.0,
                supports_reasoning: false,
                supports_caching: false,
            },
        });

        // 此测试验证工具执行路径能正确编译
        // 实际工具需要通过 ToolRegistry 注册才能执行
        let registry = Arc::new(ToolRegistry::new());
        let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

        let mut agent = AgentLoop::new(config, llm, registry, sp);
        // 没有工具调用 → 直接返回
        let output = agent.run("Hello").await.unwrap();
        assert!(!output.content.is_empty() || output.confidence < ConfidenceLevel::High);
    }

    #[test]
    fn test_classify_task_type_question() {
        assert_eq!(
            classify_task_type("What is Rust?", ""),
            TaskType::Question
        );
        assert_eq!(
            classify_task_type("How do I fix this bug?", ""),
            TaskType::Question
        );
    }

    #[test]
    fn test_classify_task_type_code_generation() {
        assert_eq!(
            classify_task_type("Implement a login endpoint", ""),
            TaskType::CodeGeneration
        );
        assert_eq!(
            classify_task_type("Create a new user model", ""),
            TaskType::CodeGeneration
        );
    }

    #[test]
    fn test_classify_task_type_code_edit() {
        assert_eq!(
            classify_task_type("Fix the bug in auth.rs", ""),
            TaskType::CodeEdit
        );
        assert_eq!(
            classify_task_type("Refactor the handler", ""),
            TaskType::CodeEdit
        );
    }

    #[test]
    fn test_classify_task_type_from_code_block() {
        // 用户没说"fix"，但输出包含代码块 → CodeEdit
        assert_eq!(
            classify_task_type("show me the code", "```rust\nfn foo() {}\n```"),
            TaskType::CodeEdit
        );
    }

    #[test]
    fn test_classify_edit_keyword_not_in_credits() {
        // "credits" 包含 "edit" 子串但不应该匹配 (词边界检查)
        assert_eq!(
            classify_task_type("show me the credits", ""),
            TaskType::Conversation
        );
    }

    #[test]
    fn test_classify_edit_keyword_not_in_expedite() {
        assert_eq!(
            classify_task_type("expedite the deployment", ""),
            TaskType::Conversation
        );
    }

    #[test]
    fn test_classify_edit_keyword_matches_edit() {
        assert_eq!(
            classify_task_type("edit the config file", ""),
            TaskType::CodeEdit
        );
    }

    #[test]
    fn test_classify_edit_keyword_matches_editing() {
        // "editing" 是整个关键词，不是 "edit" 的匹配
        assert_eq!(
            classify_task_type("I am editing the file", ""),
            TaskType::CodeEdit
        );
    }

    #[test]
    fn test_classify_edit_keyword_not_in_edition() {
        // "edition" 不包含 edit/edited/editing 作为独立词
        assert_eq!(
            classify_task_type("new edition of the book", ""),
            TaskType::Conversation
        );
    }

    #[test]
    fn test_is_word_match_positive() {
        assert!(is_word_match("edit", "please edit the file"));
        assert!(is_word_match("edit", "edit the file"));
        assert!(is_word_match("edit", "file edit"));
        assert!(is_word_match("fix", "fix the bug"));
    }

    #[test]
    fn test_is_word_match_negative() {
        assert!(!is_word_match("edit", "credits report"));
        assert!(!is_word_match("edit", "expedite delivery"));
        assert!(!is_word_match("edit", "new edition"));
        assert!(!is_word_match("fix", "prefix value"));
    }

    #[test]
    fn test_score_to_level() {
        assert_eq!(score_to_level(0.95), ConfidenceLevel::High);
        assert_eq!(score_to_level(0.9), ConfidenceLevel::High);
        assert_eq!(score_to_level(0.75), ConfidenceLevel::Medium);
        assert_eq!(score_to_level(0.6), ConfidenceLevel::Medium);
        assert_eq!(score_to_level(0.3), ConfidenceLevel::Low);
        assert_eq!(score_to_level(0.0), ConfidenceLevel::Low);
    }

    #[tokio::test]
    async fn test_with_ask_user_preserves_callback() {
        let config = test_config();
        let llm = mock_llm("test");
        let registry = Arc::new(ToolRegistry::new());
        let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

        let mut agent = AgentLoop::new(config, llm, registry, sp)
            .with_ask_user(Arc::new(|_: &str, _: &str| -> String { "ok".into() }));

        // Verify callback is stored
        assert!(agent.ask_user.is_some(), "with_ask_user MUST set the callback");

        // Verify callback works
        let answer = agent.ask_user.as_ref().unwrap()("q", "h");
        assert_eq!(answer, "ok");
    }

    #[test]
    fn test_is_retryable() {
        // ApiUnreachable is always retryable (can't construct reqwest::Error in tests)
        assert!(is_retryable(&AgentError::RateLimited {
            retry_after_seconds: 5
        }));
        assert!(!is_retryable(&AgentError::ApiKeyMissing));
        assert!(!is_retryable(&AgentError::InsufficientBalance));
        // 5xx status → retryable, 4xx → not retryable
        assert!(is_retryable(&AgentError::ApiError {
            status: 500,
            body: "boom".into()
        }));
        assert!(is_retryable(&AgentError::ApiError {
            status: 503,
            body: "unavailable".into()
        }));
        assert!(!is_retryable(&AgentError::ApiError {
            status: 400,
            body: "bad request".into()
        }));
        assert!(!is_retryable(&AgentError::ApiError {
            status: 404,
            body: "not found".into()
        }));
    }
}
