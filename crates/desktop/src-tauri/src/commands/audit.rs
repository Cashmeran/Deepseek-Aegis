// Audit logging + checkpoint snapshots — security patterns from Reasonix & DeepSeek-GUI.
// - audit.jsonl: append-only log of all tool executions (who/what/when/result)
// - checkpoints/: file snapshots before edits (rollback safety)

use std::path::PathBuf;
use std::io::Write;

/// Append a single audit entry to `.aegis/logs/audit.jsonl`.
/// Pattern from Reasonix's PauseGate audit events: every tool call is logged.
pub fn log_tool_call(cwd: &str, tool_name: &str, summary: &str, is_error: bool, elapsed_ms: u64) {
    let log_dir = PathBuf::from(cwd).join(".aegis").join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() { return; }
    let entry = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "tool": tool_name,
        "summary": summary.chars().take(200).collect::<String>(),
        "error": is_error,
        "elapsed_ms": elapsed_ms,
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("audit.jsonl")) {
        let _ = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default());
    }
}

/// Save a file snapshot before the agent modifies it.
/// Pattern from Reasonix's code/checkpoints.ts: one file per snapshot, cheap delete/restore.
pub fn checkpoint_file(cwd: &str, file_path: &str) {
    let ck_dir = PathBuf::from(cwd).join(".aegis").join("checkpoints");
    if std::fs::create_dir_all(&ck_dir).is_err() { return; }
    let src = PathBuf::from(file_path);
    if !src.exists() { return; }
    let sanitized = src.to_string_lossy().replace(['/', '\\', ':'], "_");
    let dest = ck_dir.join(format!("{}_{}.bak", sanitized, chrono::Utc::now().timestamp()));
    let _ = std::fs::copy(&src, &dest);
}
