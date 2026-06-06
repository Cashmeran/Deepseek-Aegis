//! Session persistence — global index (~/.aegis/projects.json) + per-project data (.aegis/sessions/).
//!
//! Architecture:
//!   ~/.aegis/projects.json     → global index: [{path, name, lastOpened, sessionCount}, ...]
//!   <project>/.aegis/sessions/ → per-project: index.json + <session-id>.json

use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};
use crate::events::ServerEvent;
use crate::state::SessionState;

const PROJECTS_FILE: &str = "projects.json";

fn global_aegis_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".aegis")
}

// ═══════════════════════════════════════════════════════════════
// Global project index — ~/.aegis/projects.json
// ═══════════════════════════════════════════════════════════════

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct ProjectEntry {
    path: String,
    name: String,
    #[serde(rename = "lastOpened")]
    last_opened: u64,
    #[serde(rename = "sessionCount")]
    session_count: u32,
}

fn load_projects_index() -> Vec<ProjectEntry> {
    let path = global_aegis_dir().join(PROJECTS_FILE);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_projects_index(entries: &[ProjectEntry]) {
    let path = global_aegis_dir().join(PROJECTS_FILE);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(&path, json);
    }
}

fn upsert_project(cwd: &str, name: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut entries = load_projects_index();
    if let Some(entry) = entries.iter_mut().find(|e| e.path == cwd) {
        entry.last_opened = now;
        entry.session_count = entry.session_count.saturating_add(1);
        entry.name = name.to_string();
    } else {
        entries.push(ProjectEntry {
            path: cwd.to_string(),
            name: name.to_string(),
            last_opened: now,
            session_count: 1,
        });
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.last_opened));
    save_projects_index(&entries);
}

// ═══════════════════════════════════════════════════════════════
// Tauri commands
// ═══════════════════════════════════════════════════════════════

#[tauri::command]
pub fn session_list(app: AppHandle, state: State<SessionState>) -> Result<(), String> {
    let sessions = state.list_sessions();
    app.emit("server-event", ServerEvent::SessionList { sessions }).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn recent_cwds(state: State<SessionState>, limit: Option<usize>) -> Vec<String> {
    state.list_recent_cwds(limit.unwrap_or(8).clamp(1, 20))
}

#[tauri::command]
pub fn list_skills(cwd: Option<String>) -> Vec<serde_json::Value> {
    let mut registry = aegis_core::skills::SkillRegistry::new();
    if let Some(ref dir) = cwd { let _ = registry.load_project_skills(dir); }
    let _ = registry.load_user_skills();
    registry.list().into_iter()
        .filter(|(_, _, invocable)| *invocable)
        .map(|(name, desc, _)| serde_json::json!({ "name": name, "description": desc }))
        .collect()
}

/// Load all known projects from ~/.aegis/projects.json on startup.
/// Returns project list so frontend can show them in sidebar.
#[tauri::command]
pub fn load_projects() -> Result<Vec<serde_json::Value>, String> {
    let entries = load_projects_index();
    Ok(entries.iter().map(|e| serde_json::json!({
        "path": e.path,
        "name": e.name,
        "lastOpened": e.last_opened,
        "sessionCount": e.session_count,
    })).collect())
}

/// Load sessions for a specific project from .aegis/sessions/index.json
#[tauri::command]
pub fn load_project_sessions(cwd: String) -> Result<Vec<serde_json::Value>, String> {
    let index_path = PathBuf::from(&cwd).join(".aegis").join("sessions").join("index.json");
    let entries: Vec<serde_json::Value> = std::fs::read_to_string(&index_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(entries)
}

/// Register a project in the global index (called when session starts)
#[tauri::command]
pub fn register_project(cwd: String, name: String) -> Result<(), String> {
    upsert_project(&cwd, &name);
    Ok(())
}

/// Delete a single session from .aegis/sessions/ (keeps project files intact)
#[tauri::command]
pub fn delete_session(cwd: String, session_id: String) -> Result<(), String> {
    let sessions_dir = PathBuf::from(&cwd).join(".aegis").join("sessions");
    let json_path = sessions_dir.join(format!("{session_id}.json"));
    let tmp_path = sessions_dir.join(format!("{session_id}.tmp"));
    let _ = std::fs::remove_file(&json_path);
    let _ = std::fs::remove_file(&tmp_path);
    // Update index
    let index_path = sessions_dir.join("index.json");
    let mut index: Vec<serde_json::Value> = std::fs::read_to_string(&index_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    index.retain(|e| e.get("session_id").and_then(|v| v.as_str()) != Some(&session_id));
    if let Ok(json) = serde_json::to_string_pretty(&index) {
        let _ = std::fs::write(&index_path, json);
    }
    Ok(())
}

/// Delete entire project directory (WARNING: irreversible)
#[tauri::command]
pub fn delete_project(cwd: String) -> Result<(), String> {
    // Remove from global index first
    let mut entries = load_projects_index();
    entries.retain(|e| e.path != cwd);
    save_projects_index(&entries);
    // Delete the .aegis/ directory and its contents
    let aegis_dir = PathBuf::from(&cwd).join(".aegis");
    if aegis_dir.exists() {
        std::fs::remove_dir_all(&aegis_dir)
            .map_err(|e| format!("删除 .aegis 失败: {e}"))?;
    }
    Ok(())
}

/// Check if .aegis/ exists and return saved session data for auto-restore
#[tauri::command]
pub fn check_existing_project(cwd: String) -> Result<Option<serde_json::Value>, String> {
    let aegis_dir = PathBuf::from(&cwd).join(".aegis");
    if !aegis_dir.exists() {
        return Ok(None);
    }
    let index_path = aegis_dir.join("sessions").join("index.json");
    let entries: Vec<serde_json::Value> = std::fs::read_to_string(&index_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if entries.is_empty() {
        return Ok(Some(serde_json::json!({ "hasSessions": false })));
    }
    // Load latest session
    let latest = entries.last().and_then(|e| e.get("session_id").and_then(|v| v.as_str()));
    if let Some(sid) = latest {
        let session_path = aegis_dir.join("sessions").join(format!("{sid}.json"));
        if let Ok(content) = std::fs::read_to_string(&session_path) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                return Ok(Some(serde_json::json!({
                    "hasSessions": true,
                    "sessionId": sid,
                    "messages": data.get("messages").unwrap_or(&serde_json::json!([])),
                })));
            }
        }
    }
    Ok(Some(serde_json::json!({ "hasSessions": true })))
}

/// Read a full session file from .aegis/sessions/<session_id>.json
#[tauri::command]
pub fn read_session_file(cwd: String, session_id: String) -> Result<serde_json::Value, String> {
    let path = PathBuf::from(&cwd).join(".aegis").join("sessions").join(format!("{session_id}.json"));
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read session file: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("parse session: {e}"))
}

/// Save complete frontend messages to disk (called by frontend after each turn).
/// Writes temp-file + rename for crash safety.
#[tauri::command]
pub fn save_session_messages(
    cwd: String,
    session_id: String,
    messages: Vec<serde_json::Value>,
) -> Result<(), String> {
    let sessions_dir = PathBuf::from(&cwd).join(".aegis").join("sessions");
    std::fs::create_dir_all(&sessions_dir)
        .map_err(|e| format!("mkdir sessions: {e}"))?;
    let entry = serde_json::json!({
        "session_id": session_id,
        "completed_at": chrono::Utc::now().to_rfc3339(),
        "messages": messages,
    });
    let final_path = sessions_dir.join(format!("{session_id}.json"));
    let tmp_path = sessions_dir.join(format!("{session_id}.tmp"));
    let json = serde_json::to_string_pretty(&entry)
        .map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&tmp_path, &json)
        .map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp_path, &final_path)
        .map_err(|e| format!("rename: {e}"))?;

    // Update index
    let index_path = sessions_dir.join("index.json");
    let mut index: Vec<serde_json::Value> = std::fs::read_to_string(&index_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    index.retain(|e| e.get("session_id").and_then(|v| v.as_str()) != Some(&session_id));
    index.push(serde_json::json!({
        "session_id": session_id,
        "completed_at": chrono::Utc::now().to_rfc3339(),
        "turn_count": messages.len(),
    }));
    if let Ok(json) = serde_json::to_string_pretty(&index) {
        let _ = std::fs::write(&index_path, json);
    }
    Ok(())
}
