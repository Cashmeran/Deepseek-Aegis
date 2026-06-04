use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use globset::{GlobBuilder, GlobMatcher};
use std::sync::Arc;

/// 文件模式匹配工具。支持 **/*.rs, src/**/*.ts 等 glob 语法。
pub struct GlobTool;

const SKIP_DIRS: &[&str] = &[
    "target", "node_modules", "build", "dist", "__pycache__", ".cache",
    ".git", ".svn", ".hg",
];

impl GlobTool {
    pub fn new() -> Self {
        Self
    }

    fn find_matches_with_mtime(pattern: &str, working_dir: &str) -> AgentResult<Vec<(String, std::time::SystemTime)>> {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| AgentError::ToolValidationError {
                tool: "glob".into(),
                errors: format!("Invalid glob pattern '{}': {}", pattern, e),
            })?;

        let matcher = glob.compile_matcher();
        let mut matches = Vec::new();
        let root = std::path::Path::new(working_dir);

        Self::walk_dir_with_mtime(root, root, &matcher, &mut matches);
        Ok(matches)
    }

    fn walk_dir_with_mtime(
        dir: &std::path::Path,
        root: &std::path::Path,
        matcher: &GlobMatcher,
        matches: &mut Vec<(String, std::time::SystemTime)>,
    ) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e, Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                if let Ok(relative) = path.strip_prefix(root) {
                    if matcher.is_match(relative) {
                        let mtime = entry.metadata().ok()
                            .and_then(|m| m.modified().ok())
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        matches.push((relative.display().to_string(), mtime));
                    }
                }
            } else if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') || SKIP_DIRS.contains(&name) {
                        continue;
                    }
                }
                Self::walk_dir_with_mtime(&path, root, matcher, matches);
            }
        }
    }
}

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolMetadata for GlobTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "glob".into(),
            description: "Finds files matching a glob pattern".into(),
            prompt: "Use glob to find files by pattern.\n\
                     - Supports standard glob syntax: **/*.rs, src/**/*.ts, *.md\n\
                     - Returns sorted relative paths\n\
                     - Large directories may take time; narrow your patterns".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern (e.g., '**/*.rs', 'src/**/*.ts')"
                    }
                },
                "required": ["pattern"]
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
impl Tool for GlobTool {
    async fn execute(
        self: Arc<Self>,
        tool_use: &ToolUse,
        ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage> {
        let pattern = tool_use
            .input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if pattern.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "glob".into(),
                errors: "pattern is required".into(),
            });
        }

        let start = std::time::Instant::now();
        let working_dir = ctx.working_dir.to_string_lossy().to_string();
        let mut matches = Self::find_matches_with_mtime(pattern, &working_dir)?;

        // Sort by modification time (most recent first), then alphabetically
        matches.sort_unstable_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
        });

        let elapsed = start.elapsed().as_millis() as u64;
        let total = matches.len();

        let text = if matches.is_empty() {
            format!("No files matching '{}' found in {}", pattern, working_dir)
        } else {
            let paths: Vec<String> = matches.iter()
                .map(|(p, _)| p.clone())
                .collect();
            format!(
                "Found {} file(s) matching '{}':\n{}",
                total, pattern, paths.join("\n")
            )
        };

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text { text }],
            elapsed_ms: elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_rs_files() {
        // 测试运行时 cargo 把工作目录设在 crate 根目录
        let matches = GlobTool::find_matches_with_mtime("**/*.rs", "src").unwrap();
        assert!(!matches.is_empty(), "Should find at least one .rs file: {:?}", matches);
        for (path, _) in &matches {
            assert!(path.ends_with(".rs"), "All matches should be .rs files: {}", path);
        }
    }

    #[test]
    fn test_invalid_pattern() {
        let result = GlobTool::find_matches_with_mtime("[invalid", ".");
        assert!(result.is_err());
    }
}
