use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

/// 写入前禁止覆盖的受保护文件。
const PROTECTED_WRITE_FILES: &[&str] = &[
    ".gitconfig", ".bashrc", ".zshrc", ".mcp.json", ".claude.json",
];

/// 文件写入工具。支持创建新文件和覆盖已有文件，对受保护文件拒绝写入。
pub struct FileWriteTool;

impl FileWriteTool {
    pub fn new() -> Self {
        Self
    }

    fn check_protected(path: &str) -> AgentResult<()> {
        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        for protected in PROTECTED_WRITE_FILES {
            if file_name == *protected {
                return Err(AgentError::PathTraversalBlocked {
                    path: path.into(),
                    resolved: format!("Cannot write to protected file: {}", protected),
                });
            }
        }
        if path.contains("..") {
            return Err(AgentError::PathTraversalBlocked {
                path: path.into(),
                resolved: "Path traversal detected (..)".into(),
            });
        }
        Ok(())
    }
}

impl Default for FileWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolMetadata for FileWriteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "file_write".into(),
            description: "Creates or overwrites a file on the local filesystem".into(),
            prompt: "Use file_write to create or overwrite a file.\n\
                     - Path must be within the workspace\n\
                     - Protected files (.gitconfig, .bashrc, etc.) are blocked\n\
                     - Parent directories are created automatically if needed".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["file_path", "content"]
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
impl Tool for FileWriteTool {
    async fn execute(
        self: Arc<Self>,
        tool_use: &ToolUse,
        _ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage> {
        let path = tool_use
            .input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content = tool_use
            .input
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if path.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "file_write".into(),
                errors: "file_path is required".into(),
            });
        }

        Self::check_protected(path)?;

        let start = std::time::Instant::now();

        // 创建父目录
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| AgentError::ToolExecutionError {
                tool: "file_write".into(),
                message: format!("Failed to create parent directory: {}", e),
            })?;
        }

        std::fs::write(path, content).map_err(|e| AgentError::ToolExecutionError {
            tool: "file_write".into(),
            message: format!("Failed to write file: {}", e),
        })?;

        let elapsed = start.elapsed().as_millis() as u64;
        let size = content.len();

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text {
                text: format!("Wrote {} bytes to {}", size, path),
            }],
            elapsed_ms: elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_protected_files() {
        assert!(FileWriteTool::check_protected(".gitconfig").is_err());
        assert!(FileWriteTool::check_protected(".bashrc").is_err());
        assert!(FileWriteTool::check_protected("src/main.rs").is_ok());
    }

    #[test]
    fn test_block_path_traversal() {
        assert!(FileWriteTool::check_protected("../etc/passwd").is_err());
    }
}
