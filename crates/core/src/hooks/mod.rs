//! Hook 系统 — 吸收 DS-TUI 8事件模型。
//!
//! 生命周期事件触发用户定义的 shell 命令执行。
//! 配置: .agent/hooks.json 或 skill frontmatter hooks 字段。

use std::collections::HashMap;
use std::process::Command;
use std::sync::RwLock;

/// Hook 执行器 trait。所有 hook 调度通过此接口。
pub trait HookRunner: Send + Sync {
    fn run(&self, event: HookEvent, command: &str, timeout_secs: u64);
}

/// Hook 触发事件。对齐 DS-TUI HookEvent 8 事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    MessageSubmit,
    ToolCallBefore,
    ToolCallAfter,
    ModeChange,
    OnError,
    ShellEnv,
}

impl HookEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::MessageSubmit => "message_submit",
            Self::ToolCallBefore => "tool_call_before",
            Self::ToolCallAfter => "tool_call_after",
            Self::ModeChange => "mode_change",
            Self::OnError => "on_error",
            Self::ShellEnv => "shell_env",
        }
    }
}

/// 单条 Hook 配置。
#[derive(Debug, Clone)]
pub struct Hook {
    pub event: HookEvent,
    pub command: String,
    pub timeout_secs: u64,
}

/// Hook 调度器 — 线程安全 (RwLock<HashMap>), 可 Arc 共享, 可接入 AgentLoop。
pub struct HookDispatcher {
    hooks: RwLock<HashMap<HookEvent, Vec<Hook>>>,
}

impl HookDispatcher {
    pub fn new() -> Self {
        Self { hooks: RwLock::new(HashMap::new()) }
    }

    /// 注册 hook (需要写锁)
    pub fn register(&self, event: HookEvent, command: String) {
        self.hooks.write().unwrap().entry(event).or_default().push(Hook {
            event, command, timeout_secs: 30,
        });
    }

    /// 触发事件 (读锁, 非阻塞 spawn)
    pub fn dispatch(&self, event: HookEvent) {
        let hooks = self.hooks.read().unwrap();
        if let Some(list) = hooks.get(&event) {
            for hook in list {
                let cmd = hook.command.clone();
                std::thread::spawn(move || {
                    if let Ok(mut child) = Command::new(if cfg!(windows) { "cmd" } else { "sh" })
                        .arg(if cfg!(windows) { "/C" } else { "-c" })
                        .arg(&cmd)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn() { let _ = child.wait(); }
                });
            }
        }
    }

    pub fn hook_count(&self) -> usize {
        self.hooks.read().unwrap().values().map(|v| v.len()).sum()
    }
}

impl Default for HookDispatcher {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_event_str() {
        assert_eq!(HookEvent::SessionStart.as_str(), "session_start");
    }

    #[test]
    fn test_register_and_dispatch() {
        let d = HookDispatcher::new();
        d.register(HookEvent::SessionStart, "echo started".into());
        assert_eq!(d.hook_count(), 1);
        d.dispatch(HookEvent::SessionStart);
        d.dispatch(HookEvent::SessionEnd); // no hooks, shouldn't panic
    }
}
