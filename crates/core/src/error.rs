use thiserror::Error;

/// 系统的根错误类型。所有可恢复错误通过此枚举传播。
/// 设计原则: 每个变体对应一个明确的失败域,
/// 调用方可以精确匹配并决定重试/降级/报告。
#[derive(Error, Debug)]
pub enum AgentError {
    // ── LLM 错误域 ──
    /// API密钥未设置。在首次调用LLM前检查。
    #[error("API key not configured. Set DEEPSEEK_API_KEY environment variable.")]
    ApiKeyMissing,

    /// LLM API返回错误。status_code=HTTP状态码, body=响应体前200字符。
    #[error("LLM API error (status={status}): {body}")]
    ApiError {
        status: u16,
        body: String,
    },

    /// API不可达 (DNS失败, 连接超时, TLS握手失败)。
    /// 自动重试3次, 间隔1s/2s/4s指数退避。
    #[error("LLM API unreachable after {attempts} attempts: {source}")]
    ApiUnreachable {
        attempts: u32,
        #[source]
        source: reqwest::Error,
    },

    /// API返回429 (速率限制)。等Retry-After秒后重试。
    #[error("Rate limited. Retry after {retry_after_seconds}s")]
    RateLimited { retry_after_seconds: u64 },

    /// API返回402 (余额不足)。需要用户充值。
    #[error("Insufficient balance. Please top up your account.")]
    InsufficientBalance,

    // ── 工具错误域 ──
    /// 工具未注册。可能是拼写错误或plugin未加载。
    #[error("Tool '{name}' not found in registry. Available: {available}")]
    ToolNotFound {
        name: String,
        available: String,  // 逗号分隔的已注册工具名
    },

    /// 工具执行超时。tool=工具名, timeout_ms=超时阈值。
    #[error("Tool '{tool}' timed out after {timeout_ms}ms")]
    ToolTimeout {
        tool: String,
        timeout_ms: u64,
    },

    /// 工具参数验证失败。tool=工具名, errors=具体验证错误。
    #[error("Tool '{tool}' parameter validation failed: {errors}")]
    ToolValidationError {
        tool: String,
        errors: String,  // JSON schema validation errors
    },

    /// 工具执行失败。tool=工具名, message=错误描述。
    #[error("Tool '{tool}' execution failed: {message}")]
    ToolExecutionError {
        tool: String,
        message: String,
    },

    // ── 文件操作错误域 ──
    /// 文件不存在或不可读。可能是Agent引用了不存在的文件。
    #[error("File not found: {path}")]
    FileNotFound { path: String },

    /// 路径遍历攻击检测。path包含.., ~, 或符号链接跳转。
    #[error("Path traversal blocked: {path} (resolves to {resolved})")]
    PathTraversalBlocked {
        path: String,
        resolved: String,
    },

    /// 文件过大。size_bytes=实际大小, limit_bytes=允许上限。
    #[error("File too large: {size_bytes}B exceeds limit of {limit_bytes}B")]
    FileTooLarge {
        size_bytes: u64,
        limit_bytes: u64,
    },

    // ── 上下文管理错误域 ──
    /// 上下文超过模型最大token限制, 且自动压缩无法降到阈值以下。
    #[error("Context overflow: {current_tokens}t > {max_tokens}t limit. Compaction failed.")]
    ContextOverflow {
        current_tokens: usize,
        max_tokens: usize,
    },

    /// 系统提示构建失败 (通常是模板渲染错误)。
    #[error("System prompt build failed: {0}")]
    SystemPromptError(String),

    // ── 记忆系统错误域 ──
    /// 记忆存储不可用 (SQLite文件损坏, 磁盘满等)。
    #[error("Memory store error: {0}")]
    MemoryStoreError(String),

    /// 嵌入模型加载失败 (ONNX模型文件缺失或损坏)。
    #[error("Embedding model failed to load: {0}")]
    EmbeddingError(String),

    // ── 沙箱错误域 ──
    /// 沙箱后端不可用 (非Linux, KVM未启用等)。
    #[error("Sandbox backend not available: {0}")]
    SandboxUnavailable(String),

    /// 沙箱内代码执行超时或OOM被杀。
    #[error("Sandbox execution killed: {reason}")]
    SandboxKilled { reason: String },

    // ── 配置错误域 ──
    #[error("Configuration error: {0}")]
    ConfigError(String),

    // ── 内部错误 (不可恢复, 表示bug) ──
    #[error("Internal error: {0}. This is a bug, please report.")]
    Internal(String),
}

/// Agent操作的标准Result类型。
/// Err(AgentError::Internal(...)) 表示不可恢复的bug, 不应被catch。
pub type AgentResult<T> = Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_missing_display() {
        let err = AgentError::ApiKeyMissing;
        assert_eq!(
            err.to_string(),
            "API key not configured. Set DEEPSEEK_API_KEY environment variable."
        );
    }

    #[test]
    fn test_api_error_display() {
        let err = AgentError::ApiError {
            status: 500,
            body: "Internal Server Error".into(),
        };
        assert!(err.to_string().contains("500"));
        assert!(err.to_string().contains("Internal Server Error"));
    }

    #[test]
    fn test_tool_not_found_display() {
        let err = AgentError::ToolNotFound {
            name: "nonexistent".into(),
            available: "bash,read,write".into(),
        };
        assert!(err.to_string().contains("nonexistent"));
        assert!(err.to_string().contains("bash,read,write"));
    }

    #[test]
    fn test_internal_is_error_trait() {
        let err: Box<dyn std::error::Error> = Box::new(AgentError::Internal("boom".into()));
        assert!(err.to_string().contains("boom"));
    }
}
