use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ═════════════════════════════════════════════════════════════
// 消息类型 —— 系统中所有通信的基本单位
// 参考 Anthropic Messages API 设计, 兼容 OpenAI Chat Completions
// ═════════════════════════════════════════════════════════════

/// 对话中的一条消息。整个Agent循环围绕消息流构建。
/// 每轮: UserMessage → AssistantMessage(含ToolUse) → ToolResult → 重复
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    /// 用户输入。不管是终端输入还是通过MCP传入, 都统一为此类型。
    User(UserMessage),

    /// Agent的文本回复或工具调用请求。
    Assistant(AssistantMessage),

    /// 工具执行结果。由Agent循环生成, 注入回上下文。
    ToolResult(ToolResultMessage),

    /// 系统消息。注入到对话开头, 定义Agent的行为约束。
    System(SystemMessage),
}

/// 用户消息。只包含纯文本内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    /// 消息全局唯一标识。UUID v4, 用于记忆系统的Episode追踪。
    pub id: String,

    /// 消息创建时间。UTC, 毫秒精度。
    pub timestamp: DateTime<Utc>,

    /// 文本内容。支持Markdown格式, Agent负责解析其中的指令。
    pub content: String,

    /// 元数据。不进入LLM上下文, 仅用于系统追踪。
    #[serde(default)]
    pub metadata: MessageMetadata,
}

/// Agent的回复消息。可以包含文本、工具调用, 或者两者都有。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    /// 对应UserMessage的id, 用于构建对话树。
    pub id: String,
    pub timestamp: DateTime<Utc>,

    /// 文本思考内容 (think阶段)。DeepSeek通过reasoning_content字段返回。
    pub thinking: Option<String>,

    /// 文本回复内容。对用户可见的最终输出。
    pub content: Option<String>,

    /// 工具调用请求。一个AssistantMessage可以包含多个并行工具调用。
    #[serde(default)]
    pub tool_uses: Vec<ToolUse>,

    /// 模型信息。记录哪个模型生成了此消息, 用于cost追踪和模型对比。
    pub model: Option<String>,

    /// Token使用统计。从API响应中提取, 用于cost计算。
    pub usage: Option<TokenUsage>,

    /// 停止原因。"end_turn"=正常结束, "tool_use"=等待工具执行,
    /// "max_tokens"=达到max_tokens截断, "stop_sequence"=遇到stop序列
    pub stop_reason: Option<String>,
}

/// 工具调用请求。Agent决定调用某个工具时生成。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    /// 工具调用的唯一ID。格式: "toolu_XXXX" (Anthropic兼容)
    pub id: String,

    /// 工具名称。必须在ToolRegistry中注册。
    pub name: String,

    /// 工具参数。JSON对象, 必须符合工具的input_schema。
    pub input: serde_json::Value,
}

/// 工具执行结果。Agent循环在工具执行完成后创建。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    /// 关联的ToolUse.id。Agent循环通过此ID匹配请求和响应。
    pub tool_use_id: String,

    /// 执行是否失败。true=执行异常或非零退出码, false=执行成功。
    pub is_error: bool,

    /// 结果内容。每个ContentBlock有type字段: "text" 或 "file_reference"
    pub content: Vec<ContentBlock>,

    /// 执行耗时。用于工具性能监控。
    pub elapsed_ms: u64,
}

/// 内容块。一个ToolResult可以包含多个块。
/// 例如BashTool同时返回stdout和stderr两块。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// 文本内容。大部分工具的输出。
    Text {
        text: String,
    },
    /// 文件引用。当工具输出过大(>MAX_TOOL_RESULT_CHARS=50000)时,
    /// 不嵌入内容, 而是引用磁盘文件路径。
    FileReference {
        path: String,
        preview: String,  // 前500字符预览
        total_bytes: u64,
    },
}

impl ContentBlock {
    /// 根据文本大小自动选择Text或FileReference。
    /// 阈值: MAX_TOOL_RESULT_CHARS=50000 (约15000 tokens, 按3 chars/token)
    pub fn from_text(text: &str, file_path: &str) -> Self {
        const MAX_TOOL_RESULT_CHARS: usize = 50_000;
        if text.len() > MAX_TOOL_RESULT_CHARS {
            let preview = text.chars().take(500).collect::<String>();
            ContentBlock::FileReference {
                path: file_path.to_string(),
                preview,
                total_bytes: text.len() as u64,
            }
        } else {
            ContentBlock::Text {
                text: text.to_string(),
            }
        }
    }
}

/// 系统消息。在对话开始时注入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMessage {
    /// 系统提示的完整文本。由SystemPromptBuilder组装。
    pub content: String,
}

/// 项目级规则。AGENTS.md无效(ICLR 2026)→改用结构化规则。
/// Evaluator合规检查逐条验证→违反标记为blocking issue。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRule {
    pub pattern: String,
    pub description: String,
    pub severity: RuleSeverity,
}

/// 项目规则严重级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleSeverity {
    Error,
    Warning,
}

/// Token使用统计。每次LLM API调用都返回此数据。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// 输入token数 (system prompt + 对话历史 + 工具结果)
    pub input_tokens: u64,
    /// 输出token数 (thinking + content + tool_use JSON)
    pub output_tokens: u64,
    /// 缓存命中token数。缓存命中部分按input价格的10%计费。
    pub cache_read_tokens: u64,
    /// 缓存写入token数。
    pub cache_write_tokens: u64,
}

/// 消息元数据。不在LLM上下文中, 仅用于系统追踪。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// 会话ID。整个对话会话的唯一标识。
    pub session_id: Option<String>,
    /// 来源: "terminal" (CLI), "mcp" (MCP协议), "api" (HTTP API)
    pub source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serde_roundtrip() {
        let msg = Message::User(UserMessage {
            id: "msg_001".into(),
            timestamp: Utc::now(),
            content: "Fix the bug in auth.rs".into(),
            metadata: MessageMetadata::default(),
        });
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        match decoded {
            Message::User(u) => assert_eq!(u.content, "Fix the bug in auth.rs"),
            _ => panic!("Expected User message"),
        }
    }

    #[test]
    fn test_tool_use_deserialization() {
        let json = r#"{
            "id": "toolu_001",
            "name": "bash",
            "input": {"command": "cargo test", "timeout": 120000}
        }"#;
        let tu: ToolUse = serde_json::from_str(json).unwrap();
        assert_eq!(tu.name, "bash");
        assert_eq!(tu.input["command"], "cargo test");
    }

    #[test]
    fn test_content_block_large_output() {
        let large_text = "x".repeat(50001);
        let blocks = vec![ContentBlock::from_text(&large_text, "/tmp/output.txt")];
        match &blocks[0] {
            ContentBlock::FileReference { total_bytes, .. } => {
                assert_eq!(*total_bytes, 50001);
            }
            _ => panic!("Large output should be FileReference"),
        }
    }
}
