use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use aegis_core::agent::system_prompt::SystemPromptBuilder;
use aegis_core::agent::AgentLoop;
use aegis_core::llm::client::StreamEvent;
use aegis_core::llm::deepseek::DeepSeekClient;
use aegis_core::tool_system::registry::ToolRegistry;
use aegis_core::types::config::AgentConfig;
use aegis_core::types::tool::ExecutionMode;
use aegis_memory::MemoryStore;

use crate::events::{ClientEvent, ServerEvent, SessionStatus};
use crate::state::SessionState;

/// Auto-read API key from CLI config or env var.
pub(crate) fn read_api_key_internal() -> (String, String) {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".aegis")
        .join("config.toml");
    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(config) = toml::from_str::<toml::Table>(&content) {
            let key = config.get("api_key").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let model = config.get("model").and_then(|v| v.as_str()).unwrap_or("deepseek-v4-pro").to_string();
            if !key.is_empty() { return (key, model); }
        }
    }
    (std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
     std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-pro".into()))
}

// ── Tauri event helpers ───────────────────────────────────────────

fn emit(app: &AppHandle, event: ServerEvent) -> Result<(), String> {
    app.emit("server-event", &event).map_err(|e| e.to_string())
}

// ── Agent factory — full toolkit + memory + code-graph ─────────────

pub(crate) fn build_agent(api_key: &str, model: &str, cwd: Option<&str>) -> Result<AgentLoop<DeepSeekClient>, String> {
    let llm = Arc::new(DeepSeekClient::new(api_key.into(), model)
        .map_err(|e| format!("Failed to create DeepSeek client: {e}"))?);

    let mut config = AgentConfig::default();
    config.default_model = model.to_string();
    config.verify_before_output = true; // enable confidence scoring + verification
    if let Some(dir) = cwd {
        config.workspace_dir = dir.to_string();
        log::info!("build_agent: workspace_dir = {dir}");
    } else {
        log::warn!("build_agent: no cwd, workspace_dir not set!");
    }

    let registry = Arc::new(ToolRegistry::new());
    let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

    // ── ReadTracker (shared across file tools) ──
    let read_tracker = Arc::new(aegis_tools::shared::ReadTracker::new());

    // ═══ Register ALL 31 tools (CLI parity) ═══
    use aegis_tools::*;

    // File operations
    registry.register(Arc::new(BashTool::new())).ok();
    registry.register(Arc::new(FileReadTool::new().with_read_tracker(read_tracker.clone()))).ok();
    registry.register(Arc::new(FileEditTool::new().with_read_tracker(read_tracker.clone()))).ok();
    registry.register(Arc::new(FileWriteTool::new())).ok();
    registry.register(Arc::new(ListDirTool)).ok();
    registry.register(Arc::new(FileSearchTool)).ok();

    // Search
    registry.register(Arc::new(GlobTool::new())).ok();
    registry.register(Arc::new(GrepTool::new())).ok();

    // Planning & task tracking
    registry.register(Arc::new(PlanTool)).ok();
    registry.register(Arc::new(TodoWriteTool::new())).ok();
    let task_store = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    registry.register(Arc::new(TaskCreateTool::new(task_store.clone()))).ok();
    registry.register(Arc::new(TaskGetTool::new(task_store.clone()))).ok();
    registry.register(Arc::new(TaskListTool::new(task_store.clone()))).ok();
    registry.register(Arc::new(TaskUpdateTool::new(task_store.clone()))).ok();

    // Git
    registry.register(Arc::new(GitStatusTool)).ok();
    registry.register(Arc::new(GitDiffTool)).ok();
    registry.register(Arc::new(GitLogTool)).ok();

    // Code quality
    registry.register(Arc::new(RunTestsTool)).ok();
    registry.register(Arc::new(ValidateTool)).ok();
    registry.register(Arc::new(ReviewTool)).ok();
    registry.register(Arc::new(DiagnosticsTool)).ok();
    registry.register(Arc::new(ApplyPatchTool)).ok();

    // Web
    registry.register(Arc::new(WebSearchTool::new())).ok();
    registry.register(Arc::new(WebFetchTool::new())).ok();

    // Agent interaction
    registry.register(Arc::new(AskUserTool)).ok();
    registry.register(Arc::new(RememberTool::new())).ok();

    // LSP (project-level diagnostics)
    let lsp_root = cwd.map(PathBuf::from).unwrap_or_default();
    registry.register(Arc::new(LspTool::new(lsp_root))).ok();

    // Infrastructure
    registry.register(Arc::new(ConfigTool::new())).ok();
    registry.register(Arc::new(ToolSearchTool::new())).ok();
    registry.register(Arc::new(SleepTool::new())).ok();

    // Task management
    registry.register(Arc::new(TaskOutputTool::new())).ok();
    registry.register(Arc::new(TaskStopTool::new())).ok();

    // Cron
    let cron_store = CronStore::new();
    registry.register(Arc::new(CronCreateTool::new(cron_store.clone()))).ok();
    registry.register(Arc::new(CronDeleteTool::new(cron_store.clone()))).ok();
    registry.register(Arc::new(CronListTool::new(cron_store))).ok();

    // Skill system — with backend wired to load project + user skills
    {
        let mut skills = aegis_core::skills::SkillRegistry::new();
        skills.register_bundled(
            "code-review", "Review code for bugs and improvements",
            "## Code Review\nWhen reviewing code:\n1. Check correctness\n2. Check edge cases\n3. Check security\n4. Check performance",
            Some("When user asks for code review"),
        );
        skills.register_bundled(
            "debugging", "Systematic debugging workflow",
            "## Debugging\n1. Reproduce\n2. Isolate\n3. Hypothesize\n4. Test hypothesis\n5. Fix root cause\n6. Add regression test",
            Some("When user reports a bug"),
        );
        if let Some(dir) = cwd { let _ = skills.load_project_skills(dir); }
        let sreg = Arc::new(skills);
        let sreg2 = Arc::clone(&sreg);
        let skill_tool = Arc::new(SkillTool::new().with_backend(
            Arc::new(move |name: &str, _args: &str| -> String {
                if let Some(skill) = sreg2.get(name) {
                    format!("## Skill Loaded: {}\n\n{}\n\nFollow the instructions above.", skill.name, skill.content)
                } else {
                    format!("Skill '{}' not found. Available: {}",
                        name, sreg2.list().iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>().join(", "))
                }
            })
        ));
        registry.register(skill_tool).ok();
    }

    // ── Code graph tools (CLI parity) ──
    {
        let graph_db = cwd.map(|d| PathBuf::from(d).join(".aegis").join("graph.db"))
            .unwrap_or_else(|| PathBuf::from(".aegis/graph.db"));

        use aegis_code_graph::GraphStore;
        let workspace = cwd.map(|d| d.to_string()).unwrap_or_default();
        struct ArchContextTool { db_path: PathBuf, workspace: String }
        #[async_trait::async_trait]
        impl aegis_core::types::Tool for ArchContextTool {
            async fn execute(self: Arc<Self>, tool_use: &aegis_core::types::ToolUse, _ctx: &aegis_core::types::ToolContext) -> aegis_core::error::AgentResult<aegis_core::types::ToolResultMessage> {
                let file_path = tool_use.input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
                if file_path.is_empty() {
                    return Err(aegis_core::error::AgentError::ToolValidationError { tool: "get_architectural_context".into(), errors: "file_path is required".into() });
                }
                let start = std::time::Instant::now();
                if !self.db_path.exists() {
                    return Ok(aegis_core::types::ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false, content: vec![aegis_core::types::ContentBlock::Text { text: "Codebase not indexed yet. Run scan_codebase first.".into() }], elapsed_ms: start.elapsed().as_millis() as u64 });
                }
                match aegis_code_graph::SqliteGraphStore::open(&self.db_path) {
                    Ok(store) => {
                        let resolved = if file_path.contains(':') || file_path.starts_with('/') {
                            file_path.replace('\\', "/")
                        } else {
                            format!("{}/{}", self.workspace.trim_end_matches('/').replace('\\', "/"), file_path.trim_start_matches('/'))
                        };
                        log::info!("ArchContext query: file={file_path}, workspace={}, resolved={resolved}", self.workspace);
                        let text = aegis_code_graph::get_architectural_context(&store, &resolved).unwrap_or_else(|e| format!("{e}"));
                        let truncated: String = text.lines().take(40).collect::<Vec<_>>().join("\n");
                        Ok(aegis_core::types::ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false, content: vec![aegis_core::types::ContentBlock::Text { text: truncated }], elapsed_ms: start.elapsed().as_millis() as u64 })
                    }
                    Err(e) => Ok(aegis_core::types::ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: true, content: vec![aegis_core::types::ContentBlock::Text { text: format!("DB error: {e}") }], elapsed_ms: start.elapsed().as_millis() as u64 }),
                }
            }
        }
        impl aegis_core::types::ToolMetadata for ArchContextTool {
            fn schema(&self) -> aegis_core::types::ToolSchema { aegis_core::types::ToolSchema { name: "get_architectural_context".into(), description: "Returns 1-hop architectural context: imports, callers, callees, inheritance for a file. Use BEFORE editing to understand dependencies.".into(), prompt: "Use BEFORE editing any file to understand its relationships.".into(), input_schema: serde_json::json!({"type":"object","properties":{"file_path":{"type":"string","description":"Path to source file"}},"required":["file_path"]}), } }
            fn risk_level(&self) -> aegis_core::types::RiskLevel { aegis_core::types::RiskLevel::Low }
            fn concurrency_safety(&self) -> aegis_core::types::ConcurrencySafety { aegis_core::types::ConcurrencySafety::ConcurrentSafe }
        }

        registry.register(Arc::new(ArchContextTool { db_path: graph_db.clone(), workspace: workspace.clone() })).ok();
    }

    // AgentTool — sub-agent spawning with limited tool set
    {
        let agent_llm = Arc::clone(&llm);
        let agent_registry = Arc::clone(&registry);
        let agent_sp = Arc::clone(&sp);
        let agent_config = config.clone();
        let runner: aegis_tools::agent::SubagentRunner = Arc::new(move |def: aegis_core::agent::AgentDefinition, prompt: String| {
            let llm2 = Arc::clone(&agent_llm);
            let reg2 = Arc::clone(&agent_registry);
            let _sp2 = Arc::clone(&agent_sp);
            let cfg2 = agent_config.clone();
            Box::pin(async move {
                let sub_config = {
                    let mut c = cfg2.clone();
                    if let Some(ref m) = def.model { c.default_model = m.clone(); }
                    if let Some(t) = def.max_turns { c.max_turns = t; }
                    c.verify_before_output = false;
                    c
                };
                let sub_reg = Arc::new(aegis_core::tool_system::ToolRegistry::new());
                let allow = def.tools.as_ref();
                let disallow: std::collections::HashSet<&str> = def.disallowed_tools.iter().map(|s| s.as_str()).collect();
                for name in &reg2.tool_names() {
                    let skip = if let Some(a) = allow { !a.contains(name) } else { false };
                    if skip || disallow.contains(name.as_str()) { continue; }
                    if let Some(tool) = reg2.get_clone(name) { let _ = sub_reg.register(tool); }
                }
                let sub_sp = Arc::new(aegis_core::agent::system_prompt::SystemPromptBuilder::new(sub_config.clone()));
                let start = std::time::Instant::now();
                let agent_model = def.model.clone().unwrap_or_else(|| "inherit".into());
                let mut sub = aegis_core::agent::AgentLoop::<aegis_core::llm::deepseek::DeepSeekClient>::new(
                    sub_config, llm2, sub_reg, sub_sp,
                );
                match sub.run(&prompt).await {
                    Ok(o) => aegis_core::agent::SubagentResult {
                        agent_name: def.name.clone(), output: o.content, tokens_used: 0,
                        elapsed_ms: start.elapsed().as_millis() as u64, error: None,
                        model: agent_model.clone(),
                    },
                    Err(e) => aegis_core::agent::SubagentResult {
                        agent_name: def.name, output: String::new(), tokens_used: 0,
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        error: Some(e.to_string()), model: agent_model,
                    },
                }
            })
        });
        let customs = aegis_core::agent::load_agents_dir(
            &cwd.map(std::path::PathBuf::from).unwrap_or_default()
        );
        registry.register(Arc::new(AgentTool::new().with_customs(customs).with_runner(runner))).ok();
    }

    // MCP system
    {
        let mcp_mgr = Arc::new(aegis_mcp::McpConnectionManager::new());
        if let Some(dir) = cwd {
            let mcp_config = aegis_mcp::load_mcp_config(&std::path::PathBuf::from(dir)).unwrap_or_default();
            if !mcp_config.mcp_servers.is_empty() {
                mcp_mgr.configure(mcp_config.mcp_servers);
                let mgr2 = Arc::clone(&mcp_mgr);
                tokio::spawn(async move { mgr2.connect_all(); });
            }
        }
        registry.register(Arc::new(aegis_mcp::McpToolImpl::new(Arc::clone(&mcp_mgr)))).ok();
        registry.register(Arc::new(aegis_mcp::ListMcpResourcesTool::new(Arc::clone(&mcp_mgr)))).ok();
        registry.register(Arc::new(aegis_mcp::ReadMcpResourceTool::new(mcp_mgr))).ok();
    }

    // Worktree
    registry.register(Arc::new(EnterWorktreeTool::new())).ok();
    registry.register(Arc::new(ExitWorktreeTool::new())).ok();

    // Computer use (Windows desktop automation) — only if enabled in project config
    if cwd.map_or(false, |dir| {
        let cp = std::path::Path::new(dir).join(".aegis").join("config.toml");
        std::fs::read_to_string(&cp).ok()
            .and_then(|s| toml::from_str::<toml::Table>(&s).ok())
            .and_then(|c| c.get("computer_use").and_then(|cu| cu.get("enabled")).and_then(|v| v.as_bool()))
            .unwrap_or(false)
    }) {
        crate::computer::register_all(&registry);
    }

    let tools_json = registry.get_anthropic_tools_json();
    sp.freeze_tools(&tools_json);

    let mut agent = AgentLoop::new(config, llm, registry, sp);

    // ── Project rules ──
    if let Some(dir) = cwd {
        let rules = load_project_rules(dir);
        if !rules.is_empty() {
            agent = agent.with_project_rules(rules);
        }
    }

    // ── Memory store (GAAMA causal memory) ──
    // Opens/creates .aegis/memory.db, initializes schema.
    // Retrieval is tool-mediated: the agent uses the Remember tool to
    // record and query memories. The callback provides recent context.
    let memory_db_path = project_db_path(cwd, "memory/memory.db");
    let _ = std::fs::create_dir_all(memory_db_path.parent().unwrap_or(std::path::Path::new(".")));
    if let Ok(store) = aegis_memory::SqliteMemoryStore::open(&memory_db_path) {
        let mem_store = Arc::new(store);
        let mem_retrieve = {
            let ms = Arc::clone(&mem_store);
            Arc::new(move |query: &str| -> String {
                retrieve_memory_via_store(&ms, query)
            }) as Arc<dyn Fn(&str) -> String + Send + Sync>
        };
        agent = agent.with_memory(mem_retrieve);
    }

    // ── Code graph (built on project scan, queried via LSP/file tools) ──
    if let Some(dir) = cwd {
        let graph_db_path = project_db_path(Some(dir), "code-graph/graph.db");
        if !graph_db_path.exists() {
            // Graph will be built on first project_scan in the background
            log::info!("graph.db not found, will build on first scan");
        }
    }

    Ok(agent)
}

/// Resolve a project-level DB path: .aegis/<name> if cwd set, else global ~/.aegis/<name>
fn project_db_path(cwd: Option<&str>, name: &str) -> PathBuf {
    if let Some(dir) = cwd {
        PathBuf::from(dir).join(".aegis").join(name)
    } else {
        dirs::home_dir().unwrap_or_default().join(".aegis").join(name)
    }
}

/// Load all .aegis/rules/*.md files and concatenate into a single rules string.
fn load_project_rules(cwd: &str) -> String {
    let rules_dir = std::path::Path::new(cwd).join(".aegis").join("rules");
    let mut rules = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&rules_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "md") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if !content.trim().is_empty() {
                        let name = path.file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        rules.push(format!("### {name}\n{content}"));
                    }
                }
            }
        }
    }
    rules.join("\n\n")
}

// ── Config loader (frontend calls this on startup) ────────────────

#[tauri::command]
pub fn get_config() -> Result<serde_json::Value, String> {
    let (key, model) = read_api_key_internal();
    Ok(serde_json::json!({ "apiKey": key, "model": model }))
}

// ── Main command handler ──────────────────────────────────────────

#[tauri::command]
pub async fn client_event(
    app: AppHandle,
    state: State<'_, SessionState>,
    event: ClientEvent,
) -> Result<(), String> {
    match event {
        ClientEvent::SessionList => {
            let sessions = state.list_sessions();
            emit(&app, ServerEvent::SessionList { sessions })
        }
        ClientEvent::SessionStart { title, prompt, cwd, provider: _, api_key, model, execution_mode, .. } => {
            let mut api_key = api_key.trim().to_string();
            let mut model = model.trim().to_string();
            if api_key.is_empty() { (api_key, model) = read_api_key_internal(); }
            if api_key.is_empty() {
                return emit(&app, ServerEvent::RunnerError {
                    session_id: None,
                    message: "API Key 或 Model 不能为空".into(),
                });
            }

            let mode_str = execution_mode.unwrap_or_else(|| "default".into());

            // Register project in global index
            if let Some(ref dir) = cwd {
                crate::commands::session::register_project(dir.clone(), title.clone()).ok();
            }

            let session = state.create_session(title, cwd.clone());
            state.store_provider(&session.id, crate::state::ProviderSettings {
                provider: crate::events::ProviderKind::DeepSeek,
                api_key: api_key.clone(),
                model: model.clone(),
                base_url: None,
            });
            state.store_mode(&session.id, &mode_str);
            if let Some(ref dir) = cwd { state.store_cwd(&session.id, dir); }
            let sid = session.id.clone();

            // If no prompt given, just create session without starting agent
            if prompt.trim().is_empty() {
                emit(&app, ServerEvent::SessionStatusEvent {
                    session_id: sid.clone(),
                    status: SessionStatus::Idle,
                    title: Some(session.title.clone()),
                    cwd: session.cwd.clone(),
                    error: None,
                })?;
                return Ok(());
            }

            emit(&app, ServerEvent::SessionStatusEvent {
                session_id: sid.clone(),
                status: SessionStatus::Running,
                title: Some(session.title.clone()),
                cwd: session.cwd.clone(),
                error: None,
            })?;
            emit(&app, ServerEvent::StreamUserPrompt { session_id: sid.clone(), prompt: prompt.clone() })?;

            let app_handle = app.clone();
            let session_id = sid.clone();
            let cwd_for_turn = cwd.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<SessionState>();
                let result = run_agent_turn(
                    &app_handle, &session_id, &api_key, &model, &prompt, &mode_str, cwd_for_turn.as_deref(), &state, &[],
                ).await;
                if let Err(msg) = result {
                    let _ = emit(&app_handle, ServerEvent::RunnerError {
                        session_id: Some(session_id), message: msg,
                    });
                }
            });
            Ok(())
        }
        ClientEvent::SessionContinue { session_id, prompt, messages, cwd: event_cwd } => {
            let prev_msgs = messages.unwrap_or_default();
            // Auto-init session if this is a historical project (loaded from disk without SessionStart)
            let provider = match state.get_provider(&session_id) {
                Some(p) => p,
                None => {
                    let (key, model) = read_api_key_internal();
                    if key.is_empty() {
                        return emit(&app, ServerEvent::RunnerError {
                            session_id: Some(session_id.clone()),
                            message: "API Key 未配置".into(),
                        });
                    }
                    // Auto-init session state (historical projects loaded from disk have no backend state)
                    let settings = crate::state::ProviderSettings {
                        provider: crate::events::ProviderKind::DeepSeek,
                        api_key: key,
                        model,
                        base_url: None,
                    };
                    state.store_provider(&session_id, settings.clone());
                    state.store_mode(&session_id, "default");
                    // Don't store session_id as cwd — it's not a valid path; notify_im_project sets it later
                    let title = session_id.split('/').last().unwrap_or(&session_id).to_string();
                    state.ensure_session(&session_id, title, Some(session_id.clone()));
                    settings
                }
            };
            let mode_str = state.get_mode(&session_id).unwrap_or_else(|| "default".into());
            let cwd = event_cwd.or_else(|| state.get_cwd(&session_id));

            emit(&app, ServerEvent::StreamUserPrompt { session_id: session_id.clone(), prompt: prompt.clone() })?;
            emit(&app, ServerEvent::SessionStatusEvent {
                session_id: session_id.clone(),
                status: SessionStatus::Running,
                title: None, cwd: cwd.clone(), error: None,
            })?;

            let app_handle = app.clone();
            let sid = session_id.clone();
            let key = provider.api_key.clone();
            let model = provider.model.clone();
            let prev = prev_msgs.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<SessionState>();
                let result = run_agent_turn(
                    &app_handle, &sid, &key, &model, &prompt, &mode_str, cwd.as_deref(), &state, &prev,
                ).await;
                if let Err(msg) = result {
                    let _ = emit(&app_handle, ServerEvent::RunnerError {
                        session_id: Some(sid), message: msg,
                    });
                }
            });
            Ok(())
        }
        ClientEvent::SessionStop { session_id } => {
            state.cancel_session(&session_id);
            emit(&app, ServerEvent::SessionStatusEvent {
                session_id: session_id.clone(),
                status: SessionStatus::Completed,
                title: None, cwd: None, error: None,
            })
        }
        ClientEvent::SessionDelete { session_id } => {
            state.remove_session(&session_id);
            emit(&app, ServerEvent::SessionDeleted { session_id })
        }
        ClientEvent::SessionCompact { session_id } => {
            if let Some(mut agent) = state.take_agent(&session_id) {
                let msg = agent.compact_now();
                state.put_agent(&session_id, agent);
                let _ = emit(&app, ServerEvent::StreamDelta {
                    session_id: session_id.clone(),
                    text: format!("\n[{}]\n", msg),
                });
                let _ = emit(&app, ServerEvent::SessionStatusEvent {
                    session_id: session_id.clone(),
                    status: SessionStatus::Completed,
                    title: None, cwd: None, error: None,
                });
            }
            Ok(())
        }
        ClientEvent::SessionClear { session_id } => {
            if let Some(mut agent) = state.take_agent(&session_id) {
                agent.conversation_mut().clear();
                state.put_agent(&session_id, agent);
            }
            emit(&app, ServerEvent::SessionCleared { session_id: session_id.clone() })
        }
        ClientEvent::SessionGoal { session_id, objective, criteria } => {
            let mut contract = aegis_core::agent::SprintContract::new(objective.clone());
            if let Some(ref crit) = criteria {
                for c in crit.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    contract.acceptance_criteria.push(aegis_core::agent::AcceptanceCriterion {
                        description: c.to_string(),
                        verification_command: String::new(),
                        expected_exit_code: 0,
                        expected_output_contains: None,
                    });
                }
            }
            if let Some(mut agent) = state.take_agent(&session_id) {
                let summary = contract.progress_summary();
                agent.set_goal(contract);
                state.put_agent(&session_id, agent);
                let _ = emit(&app, ServerEvent::StreamDelta {
                    session_id: session_id.clone(),
                    text: format!("\n## Goal Set\n{}\nThe agent will work towards this goal and auto-verify completion.\n", summary),
                });
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ── Agent turn — one prompt → streaming output ────────────────────

pub(crate) async fn run_agent_turn(
    app: &AppHandle,
    session_id: &str,
    api_key: &str,
    model: &str,
    prompt: &str,
    mode: &str,
    cwd: Option<&str>,
    state: &SessionState,
    prev_messages: &[serde_json::Value],
) -> Result<(), String> {
    // Guard: prevent concurrent turns for the same session
    if !state.try_start_turn(session_id) {
        return Err("该会话已有正在运行的任务".into());
    }

    // Reuse existing agent or build new one. Rebuild if cwd changed.
    let stored_cwd = state.get_cwd(session_id);
    let cwd_changed = cwd.is_some() && stored_cwd.as_deref() != cwd;
    let mut agent = match state.take_agent(session_id) {
        Some(existing) if !cwd_changed => existing,
        _ => {
            let mut a = build_agent(api_key, model, cwd)?;
            replay_conversation(&mut a, prev_messages);
            a
        }
    };

    let exec_mode = match mode {
        "chat" => ExecutionMode::Chat,
        "plan" => ExecutionMode::Plan,
        "yolo" => ExecutionMode::Yolo,
        _ => ExecutionMode::Default,
    };
    agent.set_mode(exec_mode);

    let sid = session_id.to_string();
    let app_handle = app.clone();
    const AGENT_TURN_TIMEOUT: u64 = 600;

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(AGENT_TURN_TIMEOUT),
        agent.run_streaming(prompt, &move |event: StreamEvent| {
        if !state.is_session_running(&sid) {
            return;
        }
        let _ = match event {
            StreamEvent::TextDelta(text) => {
                emit(&app_handle, ServerEvent::StreamDelta {
                    session_id: sid.clone(), text,
                })
            }
            StreamEvent::ThinkingDelta(text) => {
                emit(&app_handle, ServerEvent::StreamThinking {
                    session_id: sid.clone(), text,
                })
            }
            StreamEvent::ToolUseStart { id, name, input } => {
                emit(&app_handle, ServerEvent::StreamToolStart {
                    session_id: sid.clone(), id, name, input,
                })
            }
            StreamEvent::ToolResult { id, name, is_error, output, elapsed_ms } => {
                // Audit log + checkpoint (pattern from Reasonix)
                if let Some(ref dir) = cwd {
                    super::audit::log_tool_call(
                        dir, &name, &output, is_error, elapsed_ms,
                    );
                    if !is_error && (name == "file_edit" || name == "file_write") {
                        // Extract file path from tool input (pattern: "path" field)
                        // Snapshot is taken before the edit in the tool itself
                    }
                }
                emit(&app_handle, ServerEvent::StreamToolResult {
                    session_id: sid.clone(), id, name, is_error, output, elapsed_ms,
                })
            }
            StreamEvent::ToolProgress { tool_use_id: _, line } => {
                emit(&app_handle, ServerEvent::StreamToolProgress {
                    session_id: sid.clone(), line,
                })
            }
            StreamEvent::AskUser { question, header, options } => {
                let opts: Vec<serde_json::Value> = options.into_iter().map(|o| {
                    serde_json::json!({"label": o.label, "description": o.description})
                }).collect();
                emit(&app_handle, ServerEvent::AskUser {
                    session_id: sid.clone(), question, header, options: opts,
                })
            }
            StreamEvent::Done(resp) => {
                let cache_hit = resp.usage.cache_read_tokens as f64;
                let input = resp.usage.input_tokens as f64;
                let output = resp.usage.output_tokens as f64;
                let cache_miss = (input - cache_hit).max(0.0);
                let cost = (cache_hit * 0.025 + cache_miss * 3.0 + output * 6.0) / 1_000_000.0;
                emit(&app_handle, ServerEvent::StreamDone {
                    session_id: sid.clone(),
                    input_tokens: resp.usage.input_tokens,
                    output_tokens: resp.usage.output_tokens,
                    cache_read_tokens: resp.usage.cache_read_tokens,
                    cost,
                })
            }
        };
    }),
    ).await;

    match output {
        Ok(Ok(_)) => {
            state.end_turn(session_id);
            if let Some(dir) = cwd {
                save_session_to_disk(dir, session_id, &agent);
            }
            state.put_agent(session_id, agent);
            emit(app, ServerEvent::SessionStatusEvent {
                session_id: session_id.into(),
                status: SessionStatus::Completed,
                title: None, cwd: None, error: None,
            }).ok();
            Ok(())
        }
        Ok(Err(e)) => {
            state.end_turn(session_id);
            state.put_agent(session_id, agent);
            let _ = emit(app, ServerEvent::RunnerError {
                session_id: Some(session_id.into()), message: format!("Agent error: {e}"),
            });
            Err(format!("Agent error: {e}"))
        }
        Err(_elapsed) => {
            state.end_turn(session_id);
            state.put_agent(session_id, agent);
            let msg = format!("回合超时 ({}s)，Agent 已强制终止", AGENT_TURN_TIMEOUT);
            let _ = emit(app, ServerEvent::RunnerError {
                session_id: Some(session_id.into()), message: msg.clone(),
            });
            Err(msg)
        }
    }
}

/// Replay saved frontend messages into a fresh AgentLoop's conversation state.
fn replay_conversation(
    agent: &mut crate::state::SessionAgent,
    messages: &[serde_json::Value],
) {
    use aegis_core::types::message::Message;
    for msg in messages {
        let t = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let content = match t {
            "user_prompt" => msg.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            "assistant" => msg.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            _ => continue,
        };
        if content.is_empty() { continue; }
        let m = match t {
            "user_prompt" => Message::User(aegis_core::types::message::UserMessage {
                id: format!("replay_{}", uuid::Uuid::new_v4()),
                timestamp: chrono::Utc::now(),
                content,
                metadata: Default::default(),
            }),
            "assistant" => Message::Assistant(aegis_core::types::message::AssistantMessage {
                id: format!("replay_{}", uuid::Uuid::new_v4()),
                timestamp: chrono::Utc::now(),
                thinking: None,
                content: Some(content),
                tool_uses: vec![],
                model: None,
                usage: None,
                stop_reason: None,
            }),
            _ => continue,
        };
        agent.conversation_mut().add_message(m);
    }
}

/// Persist conversation with temp-file + rename (crash-safe, pattern from Reasonix).
/// Writes to .tmp first, then atomically renames — a crash mid-write never corrupts existing data.
fn save_session_to_disk(
    cwd: &str,
    session_id: &str,
    agent: &crate::state::SessionAgent,
) {
    use aegis_core::types::message::Message;
    let sessions_dir = std::path::Path::new(cwd).join(".aegis").join("sessions");
    if std::fs::create_dir_all(&sessions_dir).is_err() { return; }

    let messages: Vec<serde_json::Value> = agent.conversation().messages().iter().map(|msg| match msg {
        Message::User(m) => serde_json::json!({
            "type": "user_prompt", "prompt": m.content, "id": m.id,
        }),
        Message::Assistant(m) => serde_json::json!({
            "type": "assistant",
            "text": m.content.clone().unwrap_or_default(),
            "thinking": m.thinking.clone().unwrap_or_default(),
            "id": m.id,
        }),
        Message::ToolResult(tr) => serde_json::json!({
            "type": "tool_result",
            "tool_use_id": tr.tool_use_id, "is_error": tr.is_error,
            "output": tr.content.iter()
                .filter_map(|cb| match cb {
                    aegis_core::types::message::ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                }).collect::<Vec<_>>().join("\n"),
        }),
        Message::System(s) => serde_json::json!({
            "type": "system", "content": s.content,
        }),
    }).collect();

    let entry = serde_json::json!({
        "session_id": session_id,
        "completed_at": chrono::Utc::now().to_rfc3339(),
        "turn_count": messages.len(),
        "total_cost_usd": agent.conversation().total_cost_usd(),
        "messages": messages,
    });

    // Write to temp file first, then rename (crash-safe)
    let final_path = sessions_dir.join(format!("{session_id}.json"));
    let tmp_path = sessions_dir.join(format!("{session_id}.tmp"));
    if let Ok(json) = serde_json::to_string_pretty(&entry) {
        let _ = std::fs::write(&tmp_path, &json);
        let _ = std::fs::rename(&tmp_path, &final_path);
    }

    // Update index
    let index_path = sessions_dir.join("index.json");
    let mut index: Vec<serde_json::Value> = std::fs::read_to_string(&index_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    index.retain(|e| e.get("session_id").and_then(|v| v.as_str()) != Some(session_id));
    index.push(serde_json::json!({
        "session_id": session_id,
        "completed_at": chrono::Utc::now().to_rfc3339(),
        "turn_count": messages.len(),
    }));
    if let Ok(json) = serde_json::to_string_pretty(&index) {
        let _ = std::fs::write(&index_path, json);
    }
}

/// Search memory store for past insights and bugs matching the current query.
fn retrieve_memory_via_store(store: &aegis_memory::SqliteMemoryStore, query: &str) -> String {
    let keywords: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .collect();
    if keywords.is_empty() {
        return String::new();
    }

    // Walk recent episodes via consolidation API, match keywords
    if let Ok(episodes) = store.get_pending_consolidation_episodes(0, 0) {
        let matched: Vec<String> = episodes.iter()
            .filter(|ep| {
                let text = format!("{} {}", ep.user_request, ep.agent_response).to_lowercase();
                keywords.iter().any(|kw| text.contains(&kw.to_lowercase()))
            })
            .take(5)
            .map(|ep| {
                let outcome = match ep.outcome {
                    aegis_memory::EpisodeOutcome::Success => "OK",
                    aegis_memory::EpisodeOutcome::Failure => "FAIL",
                    aegis_memory::EpisodeOutcome::Unknown => "?",
                    _ => "~",
                };
                format!("- [{outcome}] Q: {}\n  A: {}",
                    ep.user_request,
                    &ep.agent_response[..ep.agent_response.len().min(200)])
            })
            .collect();
        if !matched.is_empty() {
            return format!("## Relevant Past Experience\n{}", matched.join("\n"));
        }
    }

    String::new()
}
