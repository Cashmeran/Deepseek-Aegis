pub mod constants;

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use constants::{MAX_FILE_SIZE_BYTES, MAX_LINES, PROTECTED_READ_FILES};
use std::sync::Arc;

/// Device files that would hang the process — infinite output or blocking input.
///  BLOCKED_DEVICE_PATHS.
const BLOCKED_DEVICE_PATHS: &[&str] = &[
    "/dev/zero",    // Infinite null bytes — never reaches EOF
    "/dev/random",  // Blocks until sufficient entropy available
    "/dev/urandom", // Infinite random bytes
    "/dev/full",    // Like /dev/zero but always reports "no space"
    "/dev/stdin",   // Blocks waiting for input
    "/dev/tty",     // Blocks waiting for terminal input
    "/dev/console", // System console — may block
    "/dev/stdout",  // Nonsensical to read
    "/dev/stderr",  // Nonsensical to read
    "/dev/fd/0",    // stdin alias
    "/dev/fd/1",    // stdout alias
    "/dev/fd/2",    // stderr alias
];

fn is_blocked_device(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    for blocked in BLOCKED_DEVICE_PATHS {
        if normalized == *blocked || normalized.starts_with(&format!("{}/", blocked)) {
            return true;
        }
    }
    // Linux /proc/self/fd/N and /proc/<pid>/fd/N alias for stdio
    if normalized.starts_with("/proc/") && normalized.contains("/fd/") {
        return true;
    }
    false
}

/// 文件读取工具。支持分页 (offset/limit) 和路径遍历防护。
pub struct FileReadTool {
    read_tracker: Option<Arc<crate::shared::ReadTracker>>,
}

impl FileReadTool {
    pub fn new() -> Self {
        Self { read_tracker: None }
    }

    pub fn with_read_tracker(mut self, tracker: Arc<crate::shared::ReadTracker>) -> Self {
        self.read_tracker = Some(tracker); self
    }

    fn check_protected(path: &str) -> AgentResult<()> {
        // Block dangerous device files that would hang or leak data
        if is_blocked_device(path) {
            return Err(AgentError::PathTraversalBlocked {
                path: path.into(),
                resolved: format!("Device file '{}' is blocked — infinite output or blocking input", path),
            });
        }

        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        for protected in PROTECTED_READ_FILES {
            if file_name == *protected || file_name.ends_with(&format!("/{}", protected)) {
                return Err(AgentError::PathTraversalBlocked {
                    path: path.into(),
                    resolved: format!("Access to '{}' is blocked for security", protected),
                });
            }
        }
        // Block path traversal
        if path.contains("..") {
            return Err(AgentError::PathTraversalBlocked {
                path: path.into(),
                resolved: "Path traversal detected (..)".into(),
            });
        }
        Ok(())
    }

    fn read_file(path: &str, offset: usize, limit: usize) -> AgentResult<(String, usize)> {
        let metadata = std::fs::metadata(path).map_err(|e| AgentError::FileNotFound {
            path: format!("{}: {}", path, e),
        })?;

        if metadata.len() > MAX_FILE_SIZE_BYTES {
            return Err(AgentError::FileTooLarge {
                size_bytes: metadata.len(),
                limit_bytes: MAX_FILE_SIZE_BYTES,
            });
        }

        let content = std::fs::read_to_string(path).map_err(|e| AgentError::FileNotFound {
            path: format!("{}: {}", path, e),
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let offset = offset.min(total_lines);
        let end = (offset + limit).min(total_lines);
        let selected = lines[offset..end].join("\n");

        Ok((selected, total_lines))
    }
}

impl Default for FileReadTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolMetadata for FileReadTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "file_read".into(),
            description: "Reads a file from the local filesystem".into(),
            prompt: "Use file_read to read file contents.\n\
                     - Supports offset/limit for pagination of large files\n\
                     - Max file size: 256KB\n\
                     - Sensitive files (.env, keys) are blocked".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Line number to start reading from (0-indexed)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read (default: 2000)"
                    }
                },
                "required": ["file_path"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::ConcurrentSafe
    }
}

#[async_trait]
impl Tool for FileReadTool {
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
        let offset = tool_use
            .input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let limit = tool_use
            .input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(MAX_LINES as u64) as usize;

        if path.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "file_read".into(),
                errors: "file_path is required".into(),
            });
        }

        Self::check_protected(path)?;

        let start = std::time::Instant::now();
        let (content, total_lines) = Self::read_file(path, offset, limit)?;
        let elapsed = start.elapsed().as_millis() as u64;

        // Record read for read-before-edit enforcement
        if let Some(ref tracker) = self.read_tracker {
            if offset > 0 || limit < total_lines {
                tracker.record_partial_read(path);
            } else {
                tracker.record_read(path);
            }
        }

        let header = if total_lines > offset + limit {
            format!(
                "(Showing lines {}-{} of {})\n",
                offset + 1,
                offset + content.lines().count(),
                total_lines
            )
        } else {
            String::new()
        };

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text {
                text: format!("{}{}", header, content),
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
        assert!(FileReadTool::check_protected(".env").is_err());
        assert!(FileReadTool::check_protected("/path/.gitconfig").is_err());
        assert!(FileReadTool::check_protected("id_rsa").is_err());
        assert!(FileReadTool::check_protected("src/main.rs").is_ok());
    }
}
