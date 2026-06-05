use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use aegis_core::agent::system_prompt::SystemPromptBuilder;
use aegis_core::agent::AgentLoop;
use aegis_core::llm::client::StreamEvent;
use aegis_core::llm::deepseek::DeepSeekClient;
use aegis_core::tool_system::registry::ToolRegistry;
use aegis_core::types::config::AgentConfig;

use crate::events::{ClientEvent, ServerEvent, SessionStatus};
use crate::state::SessionState;

/// Auto-read API key from CLI config or env var
fn read_api_key() -> (String, String) {
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

// ── Agent factory — one AgentLoop per session ─────────────────────

fn build_agent(api_key: &str, model: &str) -> Result<AgentLoop<DeepSeekClient>, String> {
    let llm = Arc::new(DeepSeekClient::new(api_key.into(), model)
        .map_err(|e| format!("Failed to create DeepSeek client: {e}"))?);

    let mut config = AgentConfig::default();
    config.default_model = model.to_string();

    let registry = Arc::new(ToolRegistry::new());
    let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

    // Register core tools (subset for desktop — can expand later)
    use aegis_tools::*;
    registry.register(Arc::new(BashTool::new())).ok();
    registry.register(Arc::new(FileReadTool::new())).ok();
    registry.register(Arc::new(FileEditTool::new())).ok();
    registry.register(Arc::new(FileWriteTool::new())).ok();
    registry.register(Arc::new(ListDirTool)).ok();
    registry.register(Arc::new(GlobTool::new())).ok();
    registry.register(Arc::new(GrepTool::new())).ok();
    registry.register(Arc::new(PlanTool)).ok();
    registry.register(Arc::new(TodoWriteTool::new())).ok();
    registry.register(Arc::new(GitStatusTool)).ok();
    registry.register(Arc::new(GitDiffTool)).ok();
    registry.register(Arc::new(RunTestsTool)).ok();
    registry.register(Arc::new(WebSearchTool::new())).ok();
    registry.register(Arc::new(WebFetchTool::new())).ok();

    let tools_json = registry.get_anthropic_tools_json();
    sp.freeze_tools(&tools_json);

    let mut agent = AgentLoop::new(config, llm, registry, sp);
    agent.set_mode(aegis_core::types::tool::ExecutionMode::Default);

    Ok(agent)
}

// ── Config loader (frontend calls this on startup) ────────────────

#[tauri::command]
pub fn get_config() -> Result<serde_json::Value, String> {
    let (key, model) = read_api_key();
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
        ClientEvent::SessionStart { title, prompt, cwd, provider: _, api_key, model, .. } => {
            let mut api_key = api_key.trim().to_string();
            let mut model = model.trim().to_string();
            if api_key.is_empty() { (api_key, model) = read_api_key(); }
            if api_key.is_empty() {
                return emit(&app, ServerEvent::RunnerError {
                    session_id: None,
                    message: "API Key 或 Model 不能为空".into(),
                });
            }

            let session = state.create_session(title, cwd.clone());
            let sid = session.id.clone();

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
            tauri::async_runtime::spawn(async move {
                let result = run_agent_turn(
                    &app_handle, &session_id, &api_key, &model, &prompt,
                ).await;
                if let Err(msg) = result {
                    let _ = emit(&app_handle, ServerEvent::RunnerError {
                        session_id: Some(session_id), message: msg,
                    });
                }
            });
            Ok(())
        }
        ClientEvent::SessionContinue { session_id, prompt } => {
            let provider = match state.get_provider(&session_id) {
                Some(p) => p,
                None => return emit(&app, ServerEvent::RunnerError {
                    session_id: Some(session_id), message: "Session not found".into(),
                }),
            };

            emit(&app, ServerEvent::StreamUserPrompt { session_id: session_id.clone(), prompt: prompt.clone() })?;

            let app_handle = app.clone();
            let sid = session_id.clone();
            let key = provider.api_key.clone();
            let model = provider.model.clone();
            tauri::async_runtime::spawn(async move {
                let result = run_agent_turn(
                    &app_handle, &sid, &key, &model, &prompt,
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
            state.remove_session(&session_id);
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
        _ => Ok(()),
    }
}

// ── Agent turn — one prompt → streaming output ────────────────────

async fn run_agent_turn(
    app: &AppHandle,
    session_id: &str,
    api_key: &str,
    model: &str,
    prompt: &str,
) -> Result<(), String> {
    let mut agent = build_agent(api_key, model)?;

    let sid = session_id.to_string();
    let app_handle = app.clone();

    let output = agent.run_streaming(prompt, &move |event: StreamEvent| {
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
                let cost = (resp.usage.input_tokens as f64 * 0.14
                    + resp.usage.output_tokens as f64 * 0.28) / 1_000_000.0;
                emit(&app_handle, ServerEvent::StreamDone {
                    session_id: sid.clone(),
                    input_tokens: resp.usage.input_tokens,
                    output_tokens: resp.usage.output_tokens,
                    cache_read_tokens: resp.usage.cache_read_tokens,
                    cost,
                })
            }
        };
    }).await;

    match output {
        Ok(_) => {
            emit(app, ServerEvent::SessionStatusEvent {
                session_id: session_id.into(),
                status: SessionStatus::Completed,
                title: None, cwd: None, error: None,
            }).ok();
            Ok(())
        }
        Err(e) => Err(format!("Agent error: {e}")),
    }
}
