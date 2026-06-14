use crate::error::AgentResult;
use crate::types::message::{ToolResultMessage, ToolUse};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ═════════════════════════════════════════════════════════════
// Tool trait —— 所有工具的抽象接口
// 参考 Claude Code 的 Tool base class 设计
// ═════════════════════════════════════════════════════════════

/// 工具的JSON Schema定义。用于LLM理解工具的输入参数格式。
/// 参考 JSON Schema Draft 2020-12。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    /// 工具名称。在系统提示中呈现给LLM。
    /// 命名规范: snake_case, 动词_名词。
    pub name: String,

    /// 工具的一句话描述。在系统提示中列出所有工具时使用。
    /// 长度控制在80字符以内, LLM需要快速扫描。
    pub description: String,

    /// 详细的工具使用说明。注入系统提示, 告诉LLM何时/如何使用此工具。
    /// 建议长度: 200-500词。
    pub prompt: String,

    /// 输入参数的JSON Schema。
    pub input_schema: serde_json::Value,
}

/// Tool trait —— 所有工具必须实现的异步执行接口。
/// 工具被LLM调用后由Agent循环异步执行(self是Arc指针, 支持共享引用)。
#[async_trait]
pub trait Tool: ToolMetadata + Send + Sync {
    /// 执行工具调用, 返回工具执行结果。
    async fn execute(
        self: Arc<Self>,
        tool_use: &ToolUse,
        ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage>;
}

/// Tool trait 的辅助方法。
/// 这些方法有默认实现, 工具可以选择覆盖。
pub trait ToolMetadata {
    /// 返回工具的并发安全性分类。默认ConcurrentSafe。
    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::ConcurrentSafe
    }

    /// 返回工具的风险等级。默认High（最小权限原则: 新工具必须显式声明为Low/Medium）。
    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    /// 返回工具的schema定义。实现此方法即可注册到ToolRegistry。
    fn schema(&self) -> ToolSchema;

    /// 验证输入参数是否符合input_schema。
    /// 默认实现仅检查required字段是否存在。工具可覆盖做更严格验证。
    fn validate_input(&self, input: &serde_json::Value) -> AgentResult<()> {
        let schema = self.schema();
        if let Some(required) = schema
            .input_schema
            .get("required")
            .and_then(|r| r.as_array())
        {
            for field in required {
                let field_name = field.as_str().unwrap_or("<non-string>");
                if input.get(field_name).is_none() {
                    return Err(crate::error::AgentError::ToolValidationError {
                        tool: schema.name.clone(),
                        errors: format!("Missing required field: {}", field_name),
                    });
                }
            }
        }
        Ok(())
    }
}

// ═════════════════════════════════════════════════════════════
// 工具辅助类型 —— 必须在 ToolContext 之前定义
// ═════════════════════════════════════════════════════════════

/// 工具的并发安全性声明。Agent循环在并行调度时使用此信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrencySafety {
    /// 此工具可以和其他工具并行执行, 无共享状态冲突。
    ConcurrentSafe,
    /// 此工具修改全局状态, 不能和其他ConcurrentUnsafe工具并行。
    ConcurrentUnsafe,
}

/// 权限模式。对齐 Claude Code 的 4 种用户模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// 默认模式。高风险工具 (bash, file_write, file_edit) 弹出确认，低风险直接通过。
    Default,
    /// 规划模式。仅允许只读工具 (file_read, grep, glob)，写入和网络操作拒绝。
    Plan,
    /// 全自动模式。跳过所有权限确认，危险命令检测仍然生效。
    Yolo,
    /// 对话模式。拒绝所有工具调用，仅纯文本对话。等价于 /mode chat。
    Chat,
}

/// 工具风险等级。决定是否触发权限检查。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// 低风险: 只读操作 (file_read, grep, glob)
    Low,
    /// 中风险: 网络操作 (web_fetch, web_search)
    Medium,
    /// 高风险: 写入/执行操作 (file_write, bash)
    High,
}

/// 推理强度: DeepSeek reasoning_effort 参数
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningEffort {
    Off,
    High,
    Max,
}

/// Agent 输出类型: 决定验证策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskType {
    CodeGeneration,
    CodeEdit,
    Question,
    Conversation,
}

/// Evaluator 启用模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvaluatorMode {
    Always, // 全验证
    Auto,   // 条件启用
    Never,  // 仅信心
}

/// 执行模式: 控制Agent的工具访问范围 + 权限策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    Chat,    // 无工具, 纯对话
    Plan,    // 只读工具 + plan tool, 产出计划供审批, 通过后→Yolo执行
    Default, // 完整工具集, 破坏性操作需 ask_user 审批
    Yolo,    // 完整工具集, 自动批准所有操作
}

impl ExecutionMode {
    /// Map ExecutionMode to PermissionMode for tool-level access control.
    pub fn to_permission_mode(self) -> PermissionMode {
        match self {
            ExecutionMode::Chat => PermissionMode::Chat,
            ExecutionMode::Plan => PermissionMode::Plan,
            ExecutionMode::Default => PermissionMode::Default,
            ExecutionMode::Yolo => PermissionMode::Yolo,
        }
    }
}

// ═════════════════════════════════════════════════════════════
// ToolContext —— 依赖上面的 PermissionMode
// ═════════════════════════════════════════════════════════════

/// 工具的执行上下文。Agent循环传递给每个工具调用。
/// 包含当前会话的状态、权限模式、工作目录等。
/// Ask-user callback type. Takes (question_json, header) → user_response.
pub type AskUserCallback = Arc<dyn Fn(&str, &str) -> String + Send + Sync>;

/// Newtype wrapper to make AskUserCallback Debug-able
#[derive(Clone, Default)]
pub struct DebugAskUserCb(pub Option<AskUserCallback>);
impl std::fmt::Debug for DebugAskUserCb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AskUserCb({})", self.0.is_some())
    }
}

pub struct ToolContext {
    /// 当前工作目录。所有相对路径基于此目录解析。
    pub working_dir: std::path::PathBuf,

    /// 权限模式。决定工具执行是否需要用户确认。
    pub permission_mode: PermissionMode,

    /// 会话ID。工具可以将结果关联到特定会话。
    pub session_id: String,

    /// 环境变量。Sandbox工具需要通过此字段获取配置。
    pub env: std::collections::HashMap<String, String>,

    /// 是否在沙箱中执行。true=工具调用会被SandboxManager拦截。
    pub sandbox_enabled: bool,

    /// 沙箱实例。如果启用了沙箱, Bash/FileWrite 等工具在沙箱内执行。
    #[allow(clippy::type_complexity)]
    pub sandbox: Option<std::sync::Arc<std::sync::Mutex<Box<dyn crate::types::sandbox::SandboxInstance>>>>,

    /// 工具超时。每个工具可以设置不同的超时, 覆盖全局默认。
    pub timeout_ms: u64,

    /// Ask-user callback (for ask_user tool to show TUI dialog)
    pub ask_user_cb: DebugAskUserCb,

    /// Progress callback for streaming tool output (line by line)
    #[allow(clippy::type_complexity)]
    pub progress_tx: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>>,
}

impl Clone for ToolContext {
    fn clone(&self) -> Self {
        Self {
            working_dir: self.working_dir.clone(),
            permission_mode: self.permission_mode,
            session_id: self.session_id.clone(),
            env: self.env.clone(),
            sandbox_enabled: self.sandbox_enabled,
            sandbox: self.sandbox.clone(),
            timeout_ms: self.timeout_ms,
            ask_user_cb: self.ask_user_cb.clone(),
            progress_tx: self.progress_tx.clone(),
        }
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("working_dir", &self.working_dir)
            .field("permission_mode", &self.permission_mode)
            .field("session_id", &self.session_id)
            .field("sandbox_enabled", &self.sandbox_enabled)
            .field("sandbox", &self.sandbox.as_ref().map(|_| "present"))
            .field("env", &self.env)
            .field("timeout_ms", &self.timeout_ms)
            .field("ask_user_cb", &self.ask_user_cb)
            .finish()
    }
}
