//! Agent 主循环。ReAct 模式: 思考→决策→执行→验证→回复。
//!
//! 核心创新嵌入:
//! - 三体分离 (Planner→Generator→Evaluator)
//! - SprintContract 验收契约
//! - 信心评分 — 推理链结构特征 6 权重扣分制
//! - Cache-First 上下文管理 — 缓存断点 + 6级自适应折叠
//!
//! 大型方法已拆分到子模块: run / streaming / execution / verification / retry

use crate::agent::context::ContextManager;
use crate::agent::conversation::ConversationState;
use crate::agent::harness::SprintContract;
use crate::agent::system_prompt::{HarnessPhase, SystemPromptBuilder};
use crate::llm::client::LlmClient;
use crate::llm::scorer::CodeScorer;
use crate::tool_system::registry::ToolRegistry;
use crate::tool_system::repair::ToolCallRepair;
use crate::types::config::AgentConfig;
use crate::types::message::{Message, SystemMessage};
use crate::types::tool::ExecutionMode;
use std::sync::Arc;

// Sub-modules containing the large method implementations
mod run;
mod streaming;
mod execution;
mod verification;
mod retry;

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
pub struct AgentLoop<L: LlmClient> {
    /// 全局配置
    pub(crate) config: AgentConfig,
    /// LLM 客户端 (trait, 可 Mock)
    pub(crate) llm: Arc<L>,
    /// 工具注册中心
    pub(crate) registry: Arc<ToolRegistry>,
    /// 系统提示构建器
    pub(crate) system_prompt: Arc<SystemPromptBuilder>,
    /// 上下文管理器 (折叠/缓存)
    pub(crate) context_mgr: ContextManager,
    /// 对话状态
    pub(crate) conversation: ConversationState,
    /// 当前任务的 SprintContract (Planner 在任务开始前创建)
    pub(crate) active_contract: Option<SprintContract>,
    /// 本轮是否已折叠 (防止单轮多次折叠)
    pub(crate) already_folded_this_turn: bool,
    /// 当前执行模式
    pub(crate) mode: ExecutionMode,
    /// 工具调用修复管线 (4-pass: Scavenge/Truncation/Storm/Flatten)
    pub(crate) repair: ToolCallRepair,
    /// 代码评分器 (trait in core, impl from external or RuleBasedScorer)
    pub(crate) code_scorer: Option<Arc<dyn CodeScorer>>,
    /// 记忆检索回调 (external crate 注入, core 不依赖 memory crate)
    pub(crate) memory_retrieve: Option<Arc<dyn Fn(&str) -> String + Send + Sync>>,
    /// 代码图谱查询回调 (external crate 注入, core 不依赖 code-graph crate)
    pub(crate) graph_context: Option<Arc<dyn Fn(&str) -> String + Send + Sync>>,
    /// 用户询问回调 — agent 主动暂停请求用户决策 (CLI/UI 注入)
    pub(crate) ask_user: Option<Arc<dyn Fn(&str, &str) -> String + Send + Sync>>,
    /// 沙箱实例 (sandbox crate 注入, core 不依赖 sandbox)
    pub(crate) sandbox: Option<Arc<std::sync::Mutex<Box<dyn crate::types::sandbox::SandboxInstance>>>>,
    /// 共享配置 (CLI 注入, 允许运行时 /slash 修改)
    pub(crate) shared_config: Option<Arc<std::sync::RwLock<AgentConfig>>>,
    /// Skill 系统提示注入文本
    pub(crate) skill_injection: Option<String>,
    /// 项目规则注入文本 (从 .aegis/rules/*.md 加载)
    pub(crate) project_rules: Option<String>,
    pub(crate) project_knowledge: Option<String>,
    /// 代码库概览 (codebase overview, 启动时生成)
    pub(crate) codebase_overview: Option<String>,
    /// 当前轮的任务追踪列表
    #[allow(dead_code)]
    pub(crate) active_todos: Vec<TodoItem>,
    /// 三体阶段 (Planner → Generator → Evaluator)
    pub(crate) phase: HarnessPhase,
    /// Pain6 自救计数器
    pub(crate) self_rescue_rounds: u32,
    /// Planner 阶段已执行轮数
    pub(crate) planner_turns: u32,
    /// Tool progress streaming callback
    pub(crate) tool_progress_tx: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

impl<L: LlmClient> AgentLoop<L> {
    pub fn new(
        config: AgentConfig,
        llm: Arc<L>,
        registry: Arc<ToolRegistry>,
        system_prompt: Arc<SystemPromptBuilder>,
    ) -> Self {
        let context_mgr =
            ContextManager::new(config.clone(), 4);

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
            repair: ToolCallRepair::new(),
            code_scorer: None,
            memory_retrieve: None,
            graph_context: None,
            ask_user: None,
            sandbox: None,
            shared_config: None,
            skill_injection: None,
            project_rules: None,
            project_knowledge: None,
            codebase_overview: None,
            active_todos: Vec::new(),
            phase: HarnessPhase::Generator,
            self_rescue_rounds: 0,
            planner_turns: 0,
            tool_progress_tx: None,
        }
    }

    // ── 阶段推进 ──

    fn advance_phase(&mut self, turn: u32) {
        match self.phase {
            HarnessPhase::Planner => {
                self.planner_turns += 1;
                if self.planner_turns >= 2 {
                    self.phase = HarnessPhase::Generator;
                    tracing::info!("Three-body: Planner → Generator (turn {turn})");
                }
            }
            HarnessPhase::Generator => {}
            HarnessPhase::Evaluator => {}
        }
    }

    pub fn set_phase(&mut self, phase: HarnessPhase) { self.phase = phase; }
    pub fn phase(&self) -> HarnessPhase { self.phase }

    // ── Builder 方法 ──

    pub fn with_code_scorer(mut self, scorer: Arc<dyn CodeScorer>) -> Self {
        self.code_scorer = Some(scorer); self
    }

    pub fn with_memory(mut self, retrieve: Arc<dyn Fn(&str) -> String + Send + Sync>) -> Self {
        self.memory_retrieve = Some(retrieve); self
    }

    pub fn with_graph(mut self, query: Arc<dyn Fn(&str) -> String + Send + Sync>) -> Self {
        self.graph_context = Some(query); self
    }

    pub fn with_skills(mut self, injection: String) -> Self {
        self.skill_injection = Some(injection); self
    }

    pub fn with_project_rules(mut self, rules: String) -> Self {
        self.project_rules = Some(rules); self
    }

    pub fn set_project_rules(&mut self, rules: String) {
        if rules.is_empty() { self.project_rules = None; }
        else { self.project_rules = Some(rules); }
    }

    pub fn with_project_knowledge(mut self, knowledge: String) -> Self {
        self.project_knowledge = Some(knowledge); self
    }

    pub fn set_project_knowledge(&mut self, knowledge: String) {
        if knowledge.is_empty() { self.project_knowledge = None; }
        else { self.project_knowledge = Some(knowledge); }
    }

    pub fn append_skill_injection(&mut self, injection: &str) {
        if let Some(ref mut existing) = self.skill_injection {
            existing.push_str("\n\n");
            existing.push_str(injection);
        } else {
            self.skill_injection = Some(injection.to_string());
        }
    }

    pub fn with_codebase_overview(mut self, overview: String) -> Self {
        self.codebase_overview = Some(overview); self
    }

    pub fn set_codebase_overview(&mut self, overview: String) {
        if !overview.is_empty() { self.codebase_overview = Some(overview); }
    }

    pub fn with_ask_user(mut self, cb: Arc<dyn Fn(&str, &str) -> String + Send + Sync>) -> Self {
        self.ask_user = Some(cb); self
    }

    pub fn with_sandbox(mut self, instance: Arc<std::sync::Mutex<Box<dyn crate::types::sandbox::SandboxInstance>>>) -> Self {
        self.sandbox = Some(instance); self
    }

    pub fn with_shared_config(mut self, cfg: Arc<std::sync::RwLock<AgentConfig>>) -> Self {
        self.shared_config = Some(cfg); self
    }

    // ── 配置同步 ──

    pub(crate) fn sync_config(&mut self) {
        if let Some(ref cfg) = self.shared_config {
            let c = cfg.read().unwrap_or_else(|e| e.into_inner());
            self.config.thinking_enabled = c.thinking_enabled;
            self.config.web_search_enabled = c.web_search_enabled;
            self.config.reasoning_effort = c.reasoning_effort.clone();
            self.config.verify_before_output = c.verify_before_output;
            self.config.snapshots_enabled = c.snapshots_enabled;
        }
    }

    // ── 上下文管理 ──

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

    pub(crate) fn effective_ctx_max(&self) -> usize {
        if self.config.max_context_tokens == 0 {
            self.llm.model_info().max_context_tokens * 4 / 5
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

    pub fn set_skills_injection(&mut self, text: String) {
        self.skill_injection = Some(text);
    }

    pub fn steer(&mut self, content: &str) {
        const STEER_WRAPPER: &str = "[Mid-turn steer queued by the user. Do not treat this as a new task; \
            use it only as additional guidance for the current task after completing the current step.]";
        let wrapped = format!("{}\n\n{}", STEER_WRAPPER, content);
        self.conversation.add_message(Message::System(SystemMessage { content: wrapped }));
    }

    // ── 系统提示构建 ──

    pub(crate) fn build_system_prompt(&self, user_input: &str) -> String {
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
            let rules = self.project_rules.clone();
            let knowledge = self.project_knowledge.clone();
            let mut parts: Vec<String> = Vec::new();
            if let Some(ref o) = overview { if !o.is_empty() { parts.push(o.clone()); } }
            if let Some(ref m) = memory { if !m.is_empty() { parts.push(m.clone()); } }
            if let Some(ref g) = graph { if !g.is_empty() { parts.push(g.clone()); } }
            if let Some(ref r) = rules { if !r.is_empty() { parts.push(format!("## Project Rules\n{r}")); } }
            if let Some(ref k) = knowledge { if !k.is_empty() { parts.push(format!("## Project Knowledge\n{k}")); } }
            if let Some(ref s) = skills { if !s.is_empty() { parts.push(s.clone()); } }
            if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
        };

        let mut prompt = self.system_prompt
            .build(&tool_json, merged.as_deref(), None, self.mode, &self.config.default_model);
        let phase_prompt = self.system_prompt.build_phase(self.phase);
        prompt.push_str(&phase_prompt);
        prompt
    }

    /// Inject memory context as a SystemMessage (not in system prompt — protects cache).
    pub(crate) fn inject_memory_message(&mut self, user_input: &str) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::helpers::*;
    use crate::error::{AgentError, AgentResult};
    use crate::llm::client::{LlmRequest, LlmResponse};
    use crate::types::tool::TaskType;
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
        cfg.verify_before_output = false;
        cfg.max_turns = 3;
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
        let output = agent.run("Implement a hello world program").await.unwrap();

        assert!(output.content.contains("fn main"));
        assert_eq!(output.confidence, crate::agent::output::ConfidenceLevel::High);
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

        let registry = Arc::new(ToolRegistry::new());
        let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

        let mut agent = AgentLoop::new(config, llm, registry, sp);
        let output = agent.run("Hello").await.unwrap();
        assert!(!output.content.is_empty() || output.confidence < crate::agent::output::ConfidenceLevel::High);
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
        assert_eq!(
            classify_task_type("show me the code", "```rust\nfn foo() {}\n```"),
            TaskType::CodeEdit
        );
    }

    #[test]
    fn test_classify_edit_keyword_not_in_credits() {
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
        assert_eq!(
            classify_task_type("I am editing the file", ""),
            TaskType::CodeEdit
        );
    }

    #[test]
    fn test_classify_edit_keyword_not_in_edition() {
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

    #[tokio::test]
    async fn test_with_ask_user_preserves_callback() {
        let config = test_config();
        let llm = mock_llm("test");
        let registry = Arc::new(ToolRegistry::new());
        let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

        let mut agent = AgentLoop::new(config, llm, registry, sp)
            .with_ask_user(Arc::new(|_: &str, _: &str| -> String { "ok".into() }));

        assert!(agent.ask_user.is_some(), "with_ask_user MUST set the callback");

        let answer = agent.ask_user.as_ref().unwrap()("q", "h");
        assert_eq!(answer, "ok");
    }

    #[test]
    fn test_is_retryable() {
        assert!(is_retryable(&AgentError::RateLimited {
            retry_after_seconds: 5
        }));
        assert!(!is_retryable(&AgentError::ApiKeyMissing));
        assert!(!is_retryable(&AgentError::InsufficientBalance));
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
