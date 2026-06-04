pub mod constants;
pub mod security;

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ContentBlock, ConcurrencySafety, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use constants::{DEFAULT_BASH_TIMEOUT_MS, MAX_BASH_OUTPUT_CHARS};
use std::process::Output;
use std::sync::{Arc, Mutex};

/// Background task handle — returned by run_background.
pub struct BackgroundTask {
    pub task_id: String,
    pub command: String,
    pub handle: tokio::task::JoinHandle<std::process::Output>,
}

/// Bash 工具。在子进程中执行 shell 命令。
/// 安全模型: 命令白名单 + 路径遍历防护 + 输出大小限制。
/// 支持后台任务 (run_background) 和进程树终止 (kill).
pub struct BashTool {
    #[allow(dead_code)]
    sandbox_enabled: bool,
    background_tasks: Arc<Mutex<Vec<BackgroundTask>>>,
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            sandbox_enabled: false,
            background_tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_sandbox() -> Self {
        Self {
            sandbox_enabled: true,
            background_tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Execute a command in the background. Returns task_id immediately.
    /// Results can be retrieved later via task_output.
    pub async fn run_background(
        &self,
        command: &str,
        timeout_ms: u64,
    ) -> String {
        let task_id = format!("bg-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0));
        let cmd = command.to_string();
        let cmd_clone = cmd.clone();
        let tid = task_id.clone();

        let handle = tokio::spawn(async move {
            Self::execute_sync(&cmd_clone, timeout_ms)
        });

        self.background_tasks.lock().unwrap().push(BackgroundTask {
            task_id: tid, command: cmd, handle,
        });

        task_id
    }

    /// Kill a background task by task_id.
    pub fn kill_task(&self, task_id: &str) -> bool {
        let mut tasks = self.background_tasks.lock().unwrap();
        if let Some(pos) = tasks.iter().position(|t| t.task_id == task_id) {
            let task = tasks.remove(pos);
            task.handle.abort();
            true
        } else {
            false
        }
    }

    /// Kill all processes in the process tree of a given PID.
    /// Cross-platform: taskkill /T on Windows, SIGKILL -pid on Unix.
    pub fn kill_process_tree(pid: u32) {
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
        }
        #[cfg(unix)]
        {
            unsafe { libc::kill(-(pid as i32), libc::SIGKILL); }
        }
    }

    /// Synchronous command execution (for background tasks).
    fn execute_sync(command: &str, _timeout_ms: u64) -> std::process::Output {
        let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        cmd.arg(if cfg!(windows) { "/C" } else { "-c" })
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        match cmd.output() {
            Ok(o) => o,
            Err(e) => std::process::Output {
                status: std::process::ExitStatus::default(),
                stdout: Vec::new(),
                stderr: format!("[background command failed: {}]", e).into_bytes(),
            }
        }
    }

    /// 本地执行命令 (非沙箱)。
    async fn execute_local(
        &self,
        command: &str,
        timeout_ms: u64,
    ) -> AgentResult<Output> {
        let output_fut = tokio::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .arg(if cfg!(windows) { "/C" } else { "-c" })
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .output();

        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            output_fut,
        )
        .await
        .map_err(|_| AgentError::ToolTimeout {
            tool: "bash".into(),
            timeout_ms,
        })?
        .map_err(|e| AgentError::ToolExecutionError {
            tool: "bash".into(),
            message: format!("Command execution failed: {}", e),
        })?;

        Ok(output)
    }

    /// Execute with line-by-line streaming to progress callback.
    async fn execute_local_streaming(
        progress: &std::sync::Arc<dyn Fn(String) + Send + Sync>,
        command: &str,
        timeout_ms: u64,
    ) -> AgentResult<Output> {
        let mut child = tokio::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .arg(if cfg!(windows) { "/C" } else { "-c" })
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| AgentError::ToolExecutionError {
                tool: "bash".into(),
                message: format!("Spawn failed: {}", e),
            })?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(mut out) = child.stdout.take() {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(&mut out);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        stdout.extend_from_slice(line.as_bytes());
                        progress(line.clone());
                    }
                    Err(_) => break,
                }
            }
        }
        if let Some(mut err) = child.stderr.take() {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(&mut err);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        stderr.extend_from_slice(line.as_bytes());
                        progress(line.clone());
                    }
                    Err(_) => break,
                }
            }
        }

        let status = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            child.wait(),
        )
        .await
        .map_err(|_| AgentError::ToolTimeout { tool: "bash".into(), timeout_ms })?
        .map_err(|e| AgentError::ToolExecutionError {
            tool: "bash".into(), message: format!("Wait failed: {}", e),
        })?;

        Ok(std::process::Output { status, stdout, stderr })
    }

    /// 将命令输出转换为 ContentBlock。
    fn output_to_blocks(output: Output, elapsed_ms: u64) -> Vec<ContentBlock> {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let mut blocks = Vec::with_capacity(2);

        if !stdout.is_empty() {
            if stdout.len() > MAX_BASH_OUTPUT_CHARS {
                blocks.push(ContentBlock::FileReference {
                    path: ".agent/bash_stdout.txt".into(),
                    preview: stdout.chars().take(500).collect(),
                    total_bytes: stdout.len() as u64,
                });
            } else {
                blocks.push(ContentBlock::Text { text: stdout });
            }
        }

        if !stderr.is_empty() {
            blocks.push(ContentBlock::Text {
                text: format!("[stderr]\n{}", stderr),
            });
        }

        if blocks.is_empty() {
            blocks.push(ContentBlock::Text {
                text: format!("(command completed in {}ms, exit={})", elapsed_ms, output.status.code().unwrap_or(-1)),
            });
        }

        blocks
    }
}

fn execute_in_sandbox(
    sandbox: &std::sync::Arc<std::sync::Mutex<Box<dyn aegis_core::types::sandbox::SandboxInstance>>>,
    command: &str,
) -> AgentResult<std::process::Output> {
    let mut instance = sandbox.lock().map_err(|e| AgentError::Internal(format!("sandbox lock: {e}")))?;
    let (cmd, args): (&str, &[&str]) = if cfg!(windows) {
        ("cmd", &["/C", command] as &[&str])
    } else {
        ("sh", &["-c", command] as &[&str])
    };
    let result = instance.execute(cmd, args)?;
    let mut stderr = result.stderr.into_bytes();
    let exit_code = result.exit_code;
    if exit_code != 0 { stderr.extend(format!("\n[sandbox exit code: {}]", exit_code).as_bytes()); }
    if result.killed { stderr.extend(b"\n[sandbox: process killed]"); }
    // Construct ExitStatus directly from raw exit code without shell round-trip.
    // On Unix: use std::os::unix::process::ExitStatusExt::from_raw.
    // On Windows: use std::os::windows::process::ExitStatusExt::from_raw.
    #[cfg(unix)]
    let status = {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(exit_code)
    };
    #[cfg(windows)]
    let status = {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(exit_code as u32)
    };
    Ok(std::process::Output { status, stdout: result.stdout.into_bytes(), stderr })
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolMetadata for BashTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "bash".into(),
            description: "Executes a shell command in a subprocess".into(),
            prompt: "Use bash to run shell commands: build, test, lint, or file operations.\n\
                     - Commands timeout after 120 seconds\n\
                     - Output > 50K chars is truncated to a file reference\n\
                     - Destructive commands (rm -rf, sudo, git push --force) are blocked\n\
                     - Use 'cargo test' not 'cargo test -- --nocapture'".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in milliseconds (default: 120000)"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::ConcurrentUnsafe
    }
}

#[async_trait]
impl Tool for BashTool {
    async fn execute(
        self: Arc<Self>,
        tool_use: &ToolUse,
        ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage> {
        let command = tool_use
            .input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let timeout_ms = tool_use
            .input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_BASH_TIMEOUT_MS);

        if command.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "bash".into(),
                errors: "command is required and must be a non-empty string".into(),
            });
        }

        // Multi-layer security validation ()
        let is_yolo = matches!(ctx.permission_mode, aegis_core::types::PermissionMode::Yolo);
        match security::validate_command(command, is_yolo) {
            security::SecurityVerdict::Safe | security::SecurityVerdict::ConfirmNeeded { .. } => {}
            security::SecurityVerdict::Blocked { reason } => {
                return Err(AgentError::ToolExecutionError {
                    tool: "bash".into(),
                    message: format!("Security blocked: {}", reason),
                });
            }
        }

        let start = std::time::Instant::now();

        // Use sandbox if available, otherwise execute directly
        let output = if let Some(ref sb) = ctx.sandbox {
            execute_in_sandbox(sb, command)?
        } else if let Some(ref progress) = ctx.progress_tx {
            Self::execute_local_streaming(progress, command, timeout_ms).await?
        } else {
            self.execute_local(command, timeout_ms).await?
        };

        let elapsed = start.elapsed().as_millis() as u64;
        let is_error = !output.status.success();

        let content = Self::output_to_blocks(output, elapsed);

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error,
            content,
            elapsed_ms: elapsed,
        })
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bash_echo() {
        let tool = Arc::new(BashTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 10_000, ask_user_cb: Default::default(), progress_tx: None };
        let tool_use = ToolUse {
            id: "toolu_test".into(),
            name: "bash".into(),
            input: serde_json::json!({"command": "echo hello world"}),
        };

        let result = tool.execute(&tool_use, &ctx).await.unwrap();
        assert!(!result.is_error);

        let has_hello = result.content.iter().any(|b| match b {
            ContentBlock::Text { text } => text.contains("hello world"),
            _ => false,
        });
        assert!(has_hello, "Expected 'hello world' in output");
    }

    #[tokio::test]
    async fn test_bash_error_command() {
        let tool = Arc::new(BashTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 10_000, ask_user_cb: Default::default(), progress_tx: None };
        let tool_use = ToolUse {
            id: "toolu_test".into(),
            name: "bash".into(),
            input: serde_json::json!({"command": "nonexistent_command_xyz"}),
        };

        let result = tool.execute(&tool_use, &ctx).await.unwrap();
        assert!(result.is_error, "Non-existent command should fail");
    }

    #[tokio::test]
    async fn test_bash_blocked_destructive() {
        let tool = Arc::new(BashTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 10_000, ask_user_cb: Default::default(), progress_tx: None };
        let tool_use = ToolUse {
            id: "toolu_test".into(),
            name: "bash".into(),
            input: serde_json::json!({"command": "rm -rf /important"}),
        };

        let err = tool.execute(&tool_use, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("Destructive"));
    }

    #[tokio::test]
    async fn test_bash_empty_command() {
        let tool = Arc::new(BashTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 10_000, ask_user_cb: Default::default(), progress_tx: None };
        let tool_use = ToolUse {
            id: "toolu_test".into(),
            name: "bash".into(),
            input: serde_json::json!({"command": ""}),
        };

        let err = tool.execute(&tool_use, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("required"));
    }
}
