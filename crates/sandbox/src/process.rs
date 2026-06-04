//! ProcessBackend — cross-platform process isolation.
//! Works on Windows, macOS, and Linux without special kernel features.

use crate::extension::{IsolationLevel, SandboxBackendExt, SandboxInstanceExt};
use aegis_core::error::{AgentError, AgentResult};
use aegis_core::types::sandbox::{SandboxBackend, SandboxInstance, SandboxPermissions, SandboxResult};
use std::path::Path;
use std::time::{Duration, Instant};

const SAFE_ENV_KEYS: &[&str] = &[
    "PATH", "HOME", "USER", "USERNAME", "LANG", "LC_ALL", "TZ", "TERM", "SHELL",
    "CARGO_HOME", "RUSTUP_HOME", "RUSTC", "RUST_BACKTRACE",
    "PYTHONPATH", "NODE_PATH", "GOPATH", "JAVA_HOME",
    "TMPDIR", "TEMP", "TMP",
    "SYSTEMROOT", "SYSTEMDRIVE", "COMSPEC", "PATHEXT",
    "DISPLAY", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS",
];

fn is_safe_env(key: &str) -> bool {
    if key.contains("API_KEY") || key.contains("TOKEN") || key.contains("SECRET") || key.contains("PASSWORD") {
        return false;
    }
    if key.starts_with("AWS_") || key.starts_with("GCP_") || key.starts_with("AZURE_") {
        return false;
    }
    SAFE_ENV_KEYS.iter().any(|safe| key.eq_ignore_ascii_case(safe))
}

pub struct ProcessBackend;

pub struct ProcessInstance {
    workspace: tempfile::TempDir,
    child_pid: Option<u32>,
    #[allow(dead_code)]
    started_at: Instant,
    #[allow(dead_code)]
    permissions: SandboxPermissions,
    execution_count: u32,
}

impl SandboxBackend for ProcessBackend {
    fn is_available() -> bool { true }

    fn spawn(&self, permissions: SandboxPermissions) -> Result<Box<dyn SandboxInstance>, AgentError> {
        std::fs::create_dir_all(".agent/sandboxes").ok();
        let workspace = tempfile::tempdir_in(".agent/sandboxes")
            .map_err(|e| AgentError::SandboxUnavailable(format!("tempdir: {}", e)))?;
        Ok(Box::new(ProcessInstance {
            workspace,
            child_pid: None,
            started_at: Instant::now(),
            permissions,
            execution_count: 0,
        }))
    }
}

impl SandboxBackendExt for ProcessBackend {
    fn name(&self) -> &'static str { "process" }
    fn isolation_level(&self) -> IsolationLevel { IsolationLevel::Process }
}

impl SandboxInstance for ProcessInstance {
    fn execute(&mut self, command: &str, args: &[&str]) -> Result<SandboxResult, AgentError> {
        let start = Instant::now();
        self.execution_count += 1;

        let mut cmd = std::process::Command::new(command);
        cmd.args(args)
            .current_dir(self.workspace.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        cmd.env_clear();
        for (key, val) in std::env::vars() {
            if is_safe_env(&key) {
                cmd.env(key, val);
            }
        }

        let mut child = cmd.spawn().map_err(|e| {
            AgentError::SandboxKilled { reason: format!("spawn: {}", e) }
        })?;
        self.child_pid = Some(child.id());

        let timeout = self.permissions.max_execution_time;
        let output = wait_with_timeout(&mut child, timeout)?;
        let elapsed = start.elapsed();

        Ok(SandboxResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
            elapsed,
            killed: output.status.code().is_none(),
        })
    }

    fn write_file(&mut self, path: &str, content: &str) -> Result<(), AgentError> {
        let full = self.workspace.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&full, content).map_err(|e| {
            AgentError::SandboxKilled { reason: format!("write_file: {}", e) }
        })
    }

    fn read_file(&self, path: &str) -> Result<String, AgentError> {
        let full = self.workspace.path().join(path);
        std::fs::read_to_string(&full).map_err(|e| {
            AgentError::SandboxKilled { reason: format!("read_file: {}", e) }
        })
    }

    fn checkpoint(&self, _name: &str) -> Result<(), AgentError> { Ok(()) }
    fn restore(&mut self, _name: &str) -> Result<(), AgentError> { Ok(()) }
    fn is_alive(&self) -> bool { self.child_pid.is_some() }
}

impl SandboxInstanceExt for ProcessInstance {
    fn kill(&mut self) -> AgentResult<()> {
        if let Some(pid) = self.child_pid.take() {
            #[cfg(unix)] { unsafe { libc::kill(pid as i32, libc::SIGKILL); } }
            #[cfg(windows)] {
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output();
            }
        }
        Ok(())
    }
    fn workspace_root(&self) -> &Path { self.workspace.path() }
}

fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> Result<std::process::Output, AgentError> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut output = std::process::Output { status, stdout: Vec::new(), stderr: Vec::new() };
                if let Some(mut out) = child.stdout.take() {
                    use std::io::Read;
                    out.read_to_end(&mut output.stdout).ok();
                }
                if let Some(mut err) = child.stderr.take() {
                    use std::io::Read;
                    err.read_to_end(&mut output.stderr).ok();
                }
                return Ok(output);
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AgentError::SandboxKilled { reason: "timeout".into() });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(AgentError::SandboxKilled { reason: format!("wait: {}", e) }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_env_whitelist() {
        assert!(is_safe_env("PATH"));
        assert!(!is_safe_env("DEEPSEEK_API_KEY"));
        assert!(!is_safe_env("AWS_ACCESS_KEY_ID"));
    }

    #[test]
    fn test_spawn_and_execute() {
        let backend = ProcessBackend;
        let perms = SandboxPermissions::read_only_workspace(".");
        let mut inst = backend.spawn(perms).unwrap();
        let (cmd, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd", &["/c", "echo", "hello"] as &[&str])
        } else {
            ("echo", &["hello"] as &[&str])
        };
        let result = inst.execute(cmd, args).unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn test_write_and_read_file() {
        let backend = ProcessBackend;
        let perms = SandboxPermissions::read_write_workspace(".");
        let mut inst = backend.spawn(perms).unwrap();
        inst.write_file("test.txt", "hello sandbox").unwrap();
        assert_eq!(inst.read_file("test.txt").unwrap(), "hello sandbox");
    }

    #[test]
    fn test_isolation_level() {
        assert_eq!(ProcessBackend.isolation_level(), IsolationLevel::Process);
    }
}
