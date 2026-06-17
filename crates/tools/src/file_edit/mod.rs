pub mod constants;

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use constants::MAX_OLD_STRING_LEN;
use std::sync::Arc;

/// 写入前禁止覆盖的受保护文件。
const PROTECTED_EDIT_FILES: &[&str] = &[
    ".gitconfig", ".bashrc", ".zshrc", ".mcp.json", ".claude.json",
];

/// Maximum editable file size: 1 MiB (aligned with CC's 1 GiB but conservative for safety).
const MAX_EDIT_FILE_SIZE: u64 = 1_048_576;

/// 文件编辑工具。使用 unified diff 风格的 old_string → new_string 替换。
/// 参考 标准 Edit 工具设计: old_string 必须在文件中出现恰好 1 次。
///
/// 安全增强 (CC aligned):
/// - 读前检查：文件必须先被 FileReadTool 读过才能编辑
/// - 编码检测：自动检测 UTF-8/UTF-16LE BOM，规范化 \r\n → \n
/// - 文件大小保护：拒绝编辑超大文件
/// - 空 old_string：创建新文件（若不存在）或清空文件
pub struct FileEditTool {
    read_tracker: Option<Arc<crate::shared::ReadTracker>>,
}

impl FileEditTool {
    pub fn new() -> Self {
        Self { read_tracker: None }
    }

    pub fn with_read_tracker(mut self, tracker: Arc<crate::shared::ReadTracker>) -> Self {
        self.read_tracker = Some(tracker); self
    }

    /// Detect encoding from BOM and normalize line endings to \n.
    /// Returns (content, was_utf16le).
    fn decode_content(bytes: &[u8]) -> (String, bool) {
        // UTF-16LE BOM: 0xFF 0xFE
        if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
            let utf16: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            let decoded = String::from_utf16_lossy(&utf16).replace("\r\n", "\n");
            return (decoded, true);
        }
        // Default: UTF-8
        let decoded = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
        (decoded.to_string(), false)
    }

    fn check_protected(path: &str) -> AgentResult<()> {
        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        for protected in PROTECTED_EDIT_FILES {
            if file_name == *protected {
                return Err(AgentError::PathTraversalBlocked {
                    path: path.into(),
                    resolved: format!("Cannot edit protected file: {}", protected),
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

    /// 执行字符串替换。old_string 必须恰好出现 1 次，new_string 必须不同于 old_string。
    fn apply_edit(content: &str, old_string: &str, new_string: &str, replace_all: bool) -> AgentResult<String> {
        // Empty old_string + empty file → just return new_string (full overwrite)
        if old_string.is_empty() && content.is_empty() {
            if old_string == new_string {
                return Err(AgentError::ToolValidationError {
                    tool: "file_edit".into(),
                    errors: "new_string must differ from old_string".into(),
                });
            }
            return Ok(new_string.to_string());
        }
        if old_string.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "file_edit".into(),
                errors: "old_string must be non-empty when editing a non-empty file".into(),
            });
        }
        if old_string.len() > MAX_OLD_STRING_LEN {
            return Err(AgentError::ToolValidationError {
                tool: "file_edit".into(),
                errors: format!(
                    "old_string too long ({} chars, max {})",
                    old_string.len(),
                    MAX_OLD_STRING_LEN
                ),
            });
        }
        if old_string == new_string {
            return Err(AgentError::ToolValidationError {
                tool: "file_edit".into(),
                errors: "new_string must differ from old_string".into(),
            });
        }

        if replace_all {
            if !content.contains(old_string) {
                return Err(AgentError::ToolExecutionError {
                    tool: "file_edit".into(),
                    message: "old_string not found in file".into(),
                });
            }
            Ok(content.replace(old_string, new_string))
        } else {
            let count = content.match_indices(old_string).count();
            if count == 0 {
                return Err(AgentError::ToolExecutionError {
                    tool: "file_edit".into(),
                    message: "old_string not found in file".into(),
                });
            }
            if count > 1 {
                return Err(AgentError::ToolExecutionError {
                    tool: "file_edit".into(),
                    message: format!(
                        "old_string appears {} times in file — must be unique. \
                         Add more context to disambiguate, or use replace_all=true.",
                        count
                    ),
                });
            }
            Ok(content.replacen(old_string, new_string, 1))
        }
    }
}

impl Default for FileEditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolMetadata for FileEditTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "file_edit".into(),
            description: "Performs exact string replacements in an existing file".into(),
            prompt: "Use file_edit for targeted text replacements in files.\n\
                     - old_string must appear exactly ONCE in the file (or use replace_all=true)\n\
                     - old_string and new_string must differ\n\
                     - Use sufficient context around the change to make old_string unique\n\
                     - Prefer editing existing files over writing new ones".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact text to replace"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The text to replace it with"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace all occurrences (default: false, requires unique match)"
                    }
                },
                "required": ["file_path", "old_string", "new_string"]
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
impl Tool for FileEditTool {
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
        let old_string = tool_use
            .input
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new_string = tool_use
            .input
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let replace_all = tool_use
            .input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if path.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "file_edit".into(),
                errors: "file_path is required".into(),
            });
        }

        Self::check_protected(path)?;

        let start = std::time::Instant::now();

        // Read-before-edit enforcement: file must have been read this session
        if let Some(ref tracker) = self.read_tracker {
            if !tracker.has_been_read(path) {
                return Err(AgentError::ToolExecutionError {
                    tool: "file_edit".into(),
                    message: format!(
                        "File '{}' has not been read yet this session. Read it first using file_read before editing.",
                        path
                    ),
                });
            }
        }

        // Concurrent modification check — reject if file was modified since last read
        if let Some(ref tracker) = self.read_tracker {
            if tracker.was_modified_since_read(path).unwrap_or(false) {
                return Err(AgentError::ToolExecutionError {
                    tool: "file_edit".into(),
                    message: format!(
                        "File '{}' has been modified since it was last read. Read it again before editing.",
                        path
                    ),
                });
            }
        }

        // Try to read existing file with encoding detection
        let (content, file_exists) = match std::fs::read(path) {
            Ok(bytes) => {
                if bytes.len() as u64 > MAX_EDIT_FILE_SIZE {
                    return Err(AgentError::FileTooLarge {
                        size_bytes: bytes.len() as u64,
                        limit_bytes: MAX_EDIT_FILE_SIZE,
                    });
                }
                let (decoded, _) = Self::decode_content(&bytes);
                (decoded, true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Suggest similar files if the target doesn't exist
                if let Some(similar) = crate::shared::find_similar_file(path) {
                    return Err(AgentError::FileNotFound {
                        path: format!("{} — did you mean '{}'?", path, similar),
                    });
                }
                (String::new(), false)
            }
            Err(e) => {
                return Err(AgentError::FileNotFound {
                    path: format!("{}: {}", path, e),
                });
            }
        };

        // Empty old_string with no existing file → create new file
        if old_string.is_empty() && !file_exists {
            std::fs::write(path, new_string).map_err(|e| AgentError::ToolExecutionError {
                tool: "file_edit".into(),
                message: format!("Failed to create file: {}", e),
            })?;
            let elapsed = start.elapsed().as_millis() as u64;
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(),
                is_error: false,
                content: vec![ContentBlock::Text {
                    text: format!("Created {} — {} bytes written", path, new_string.len()),
                }],
                elapsed_ms: elapsed,
            });
        }

        // Empty old_string with existing file → overwrite entire file
        let actual_old = if old_string.is_empty() {
            String::new() // Replace empty → full overwrite
        } else {
            old_string.to_string()
        };

        let edited = Self::apply_edit(&content, &actual_old, new_string, replace_all)?;

        std::fs::write(path, edited.as_bytes()).map_err(|e| AgentError::ToolExecutionError {
            tool: "file_edit".into(),
            message: format!("Failed to write file: {}", e),
        })?;

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text {
                text: format!("Edited {} — {} bytes written", path, edited.len()),
            }],
            elapsed_ms: elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_replace() {
        let result = FileEditTool::apply_edit("hello world", "world", "Rust", false).unwrap();
        assert_eq!(result, "hello Rust");
    }

    #[test]
    fn test_replace_all() {
        let result = FileEditTool::apply_edit("a a a", "a", "b", true).unwrap();
        assert_eq!(result, "b b b");
    }

    #[test]
    fn test_duplicate_without_replace_all_fails() {
        let result = FileEditTool::apply_edit("foo bar foo", "foo", "baz", false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("appears 2 times"));
    }

    #[test]
    fn test_not_found_fails() {
        let result = FileEditTool::apply_edit("hello", "world", "xxx", false);
        assert!(result.is_err());
    }

    #[test]
    fn test_same_string_fails() {
        let result = FileEditTool::apply_edit("hello", "hello", "hello", false);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_old_string_fails() {
        let result = FileEditTool::apply_edit("hello", "", "x", false);
        assert!(result.is_err());
    }

    #[test]
    fn test_block_protected_files() {
        assert!(FileEditTool::check_protected(".gitconfig").is_err());
        assert!(FileEditTool::check_protected(".bashrc").is_err());
        assert!(FileEditTool::check_protected(".mcp.json").is_err());
        assert!(FileEditTool::check_protected("src/main.rs").is_ok());
        assert!(FileEditTool::check_protected("Cargo.toml").is_ok());
    }

    #[test]
    fn test_block_path_traversal() {
        assert!(FileEditTool::check_protected("../etc/passwd").is_err());
        assert!(FileEditTool::check_protected("../../.ssh/id_rsa").is_err());
    }
}
