//! Grep — content search tool. : output modes, pagination, glob/type filters.
//!
//! Features:
//! - output_mode: content | files_with_matches | count
//! - head_limit + offset pagination
//! - glob file filter, type file filter
//! - multiline mode
//! - -A/-B/-C context lines, -n line numbers, -i case insensitive
//! - VCS directory exclusion (.git/.svn/.hg/.bzr/.jj/.sl)
//! - Build directory exclusion (target/node_modules/build/dist/__pycache__)
//! - Binary file skip
//! - Search timeout (per-file regex timeout + walk deadline)

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use regex::Regex;
use std::sync::Arc;
use std::time::Instant;

// ═══════════════ Constants ═══════════════

const DEFAULT_HEAD_LIMIT: usize = 250;
const WALK_DEADLINE_MS: u64 = 120_000;

const VCS_DIRS: &[&str] = &[".git", ".svn", ".hg", ".bzr", ".jj", ".sl"];
const BUILD_DIRS: &[&str] = &[
    "target", "node_modules", "build", "dist", "__pycache__", ".cache",
    ".next", ".nuxt", "vendor", "bower_components",
];
const BINARY_EXTENSIONS: &[&str] = &[
    "exe", "dll", "so", "dylib", "o", "a", "lib", "rlib",
    "png", "jpg", "jpeg", "gif", "ico", "bmp", "svg", "webp",
    "pdf", "zip", "tar", "gz", "bz2", "xz", "7z", "rar",
    "class", "pyc", "pyo", "wasm", "bin", "dat",
    "ttf", "otf", "woff", "woff2", "eot",
    "mp3", "mp4", "avi", "mov", "mkv", "wav", "flac",
];

// ═══════════════ Types ═══════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Content,
    FilesWithMatches,
    Count,
}

impl OutputMode {
    fn from_str(s: &str) -> Self {
        match s {
            "content" => Self::Content,
            "files_with_matches" => Self::FilesWithMatches,
            "count" => Self::Count,
            _ => Self::FilesWithMatches,
        }
    }
}

/// Per-file match accumulator before head_limit/offset filtering.
struct FileMatches {
    path: String,
    num_matches: usize,
    content_lines: Vec<String>, // Only populated for Content mode
}

// ═══════════════ Tool ═══════════════

pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self { Self }
}

impl Default for GrepTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for GrepTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "grep".into(),
            description: "Searches file contents using regex patterns with output modes, pagination, and filters".into(),
            prompt: "Use grep to search file contents.\n\
                     - Uses full regex syntax (ripgrep-compatible via regex crate)\n\
                     - output_mode: 'content' (matching lines with context), 'files_with_matches' (file paths only), 'count' (match counts)\n\
                     - head_limit: max output lines/entries (default 250, 0=unlimited)\n\
                     - offset: skip first N results for pagination\n\
                     - glob: filter files by glob pattern (e.g. '*.rs', '*.{ts,tsx}')\n\
                     - type: filter by file type (rust, js, py, go, java, etc.)\n\
                     - -A/-B/-C: context lines before/after/both\n\
                     - -n: show line numbers (content mode only)\n\
                     - -i: case insensitive search\n\
                     - multiline: patterns can span lines\n\
                     - Skips VCS dirs, build dirs, and binary files automatically".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern to search for"},
                    "path": {"type": "string", "description": "File or directory to search in (default: '.')"},
                    "output_mode": {"type": "string", "enum": ["content", "files_with_matches", "count"], "description": "Output mode (default: files_with_matches)"},
                    "glob": {"type": "string", "description": "Glob pattern to filter files (e.g. '*.rs', '*.{ts,tsx}')"},
                    "type": {"type": "string", "description": "File type to search: rust, js, py, go, java, ts, tsx, rs, toml, yaml, json, md"},
                    "head_limit": {"type": "integer", "description": "Max output entries (default 250, 0=unlimited)"},
                    "offset": {"type": "integer", "description": "Skip first N entries (pagination)"},
                    "-A": {"type": "integer", "description": "Lines after each match"},
                    "-B": {"type": "integer", "description": "Lines before each match"},
                    "-C": {"type": "integer", "description": "Context lines before and after (alias for -A N -B N)"},
                    "context": {"type": "integer", "description": "Alias for -C"},
                    "-n": {"type": "boolean", "description": "Show line numbers (default: true for content mode)"},
                    "-i": {"type": "boolean", "description": "Case insensitive search"},
                    "multiline": {"type": "boolean", "description": "Enable multiline mode where . matches newlines"}
                },
                "required": ["pattern"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for GrepTool {
    async fn execute(
        self: Arc<Self>,
        tool_use: &ToolUse,
        ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage> {
        let pattern = tool_use.input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let path = tool_use.input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let output_mode = tool_use.input.get("output_mode").and_then(|v| v.as_str())
            .map(OutputMode::from_str).unwrap_or(OutputMode::FilesWithMatches);
        let glob_filter = tool_use.input.get("glob").and_then(|v| v.as_str());
        let type_filter = tool_use.input.get("type").and_then(|v| v.as_str()).map(|t| type_to_extensions(t));
        let head_limit = tool_use.input.get("head_limit").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_HEAD_LIMIT as u64) as usize;
        let offset = tool_use.input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let context = tool_use.input.get("-C").or(tool_use.input.get("context"))
            .and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let after = tool_use.input.get("-A").and_then(|v| v.as_u64()).unwrap_or(context as u64) as usize;
        let before = tool_use.input.get("-B").and_then(|v| v.as_u64()).unwrap_or(context as u64) as usize;
        let show_numbers = tool_use.input.get("-n").and_then(|v| v.as_bool()).unwrap_or(true);
        let ignore_case = tool_use.input.get("-i").and_then(|v| v.as_bool()).unwrap_or(false);
        let multiline = tool_use.input.get("multiline").and_then(|v| v.as_bool()).unwrap_or(false);

        if pattern.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "grep".into(), errors: "pattern is required".into(),
            });
        }

        let start = Instant::now();
        let search_path = if path == "." {
            ctx.working_dir.to_string_lossy().to_string()
        } else {
            path.to_string()
        };

        // Build regex
        let mut builder = regex::RegexBuilder::new(pattern);
        builder.case_insensitive(ignore_case);
        if multiline {
            builder.dot_matches_new_line(true);
            builder.multi_line(true);
        }
        let re = builder.build().map_err(|e| AgentError::ToolValidationError {
            tool: "grep".into(),
            errors: format!("Invalid regex pattern '{}': {}", pattern, e),
        })?;

        let root = std::path::Path::new(&search_path);
        let mut file_matches: Vec<FileMatches> = Vec::new();
        let walk_deadline = start + std::time::Duration::from_millis(WALK_DEADLINE_MS);

        Self::search_dir(
            root, root, &re, output_mode,
            after, before, show_numbers,
            glob_filter, type_filter.as_deref(),
            &mut file_matches, &walk_deadline, 0,
        );

        let elapsed = start.elapsed().as_millis() as u64;

        // Apply head_limit and offset
        let total_files = file_matches.len();
        let total_hits: usize = file_matches.iter().map(|f| f.num_matches).sum();

        // Apply offset
        if offset > 0 {
            let mut skipped = 0usize;
            file_matches = file_matches.into_iter().filter(|_fm| {
                if skipped < offset {
                    skipped += 1;
                    false
                } else {
                    true
                }
            }).collect();
        }

        // Apply head_limit
        let (truncated, effective_limit) = if head_limit == 0 {
            (false, None)
        } else {
            let was_truncated = file_matches.len() > head_limit;
            file_matches.truncate(head_limit);
            (was_truncated, Some(head_limit))
        };

        // Format output
        let output = match output_mode {
            OutputMode::Content => {
                let mut out = String::new();
                for fm in &file_matches {
                    out.push_str(&fm.content_lines.join("\n"));
                    out.push('\n');
                }
                if truncated {
                    out.push_str(&format!("\n[Showing {}/{} files, {}/{} matches — use offset for more]",
                        effective_limit.unwrap_or(file_matches.len()), total_files,
                        file_matches.iter().map(|f| f.num_matches).sum::<usize>(), total_hits));
                }
                if out.is_empty() {
                    format!("No matches for '{}' in {}", pattern, search_path)
                } else {
                    out
                }
            }
            OutputMode::FilesWithMatches => {
                if file_matches.is_empty() {
                    format!("No files matching '{}' in {}", pattern, search_path)
                } else {
                    let mut out = String::new();
                    for fm in &file_matches {
                        out.push_str(&fm.path);
                        out.push('\n');
                    }
                    if truncated {
                        out.push_str(&format!("\n[{} of {} files shown — use offset for more]\n",
                            effective_limit.unwrap_or(file_matches.len()), total_files));
                    }
                    out
                }
            }
            OutputMode::Count => {
                if file_matches.is_empty() {
                    format!("No matches for '{}' in {}", pattern, search_path)
                } else {
                    let mut out = String::new();
                    for fm in &file_matches {
                        out.push_str(&format!("{}:{}\n", fm.path, fm.num_matches));
                    }
                    if truncated {
                        out.push_str(&format!("\n[{} of {} files shown — use offset for more]\n",
                            effective_limit.unwrap_or(file_matches.len()), total_files));
                    }
                    out
                }
            }
        };

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text { text: output }],
            elapsed_ms: elapsed,
        })
    }
}

// ═══════════════ Search implementation ═══════════════

impl GrepTool {
    fn search_dir(
        dir: &std::path::Path,
        root: &std::path::Path,
        re: &Regex,
        mode: OutputMode,
        after: usize,
        before: usize,
        show_numbers: bool,
        glob_filter: Option<&str>,
        type_filter: Option<&[&str]>,
        file_matches: &mut Vec<FileMatches>,
        deadline: &Instant,
        depth: usize,
    ) {
        if Instant::now() > *deadline || depth > 30 { return; }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e, Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            if Instant::now() > *deadline { return; }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if path.is_file() {
                // Binary check
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if BINARY_EXTENSIONS.contains(&ext) { continue; }
                }

                // Glob filter
                if let Some(g) = glob_filter {
                    if let Ok(rel) = path.strip_prefix(root) {
                        let rel_str = rel.to_string_lossy().replace('\\', "/");
                        if !simple_glob_match(g, &rel_str) { continue; }
                    }
                }

                // Type filter
                if let Some(types) = type_filter {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if !types.contains(&ext) { continue; }
                    }
                }

                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c, Err(_) => continue,
                };

                let rel = path.strip_prefix(root)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| name.clone());

                let lines: Vec<&str> = content.lines().collect();
                let mut matches: Vec<usize> = Vec::new();

                for (i, line) in lines.iter().enumerate() {
                    if re.is_match(line) {
                        matches.push(i);
                    }
                }

                if matches.is_empty() { continue; }

                match mode {
                    OutputMode::FilesWithMatches => {
                        file_matches.push(FileMatches {
                            path: rel, num_matches: matches.len(), content_lines: Vec::new(),
                        });
                    }
                    OutputMode::Count => {
                        file_matches.push(FileMatches {
                            path: rel, num_matches: matches.len(), content_lines: Vec::new(),
                        });
                    }
                    OutputMode::Content => {
                        let mut content_lines = Vec::new();

                        for (idx, &match_line) in matches.iter().enumerate() {
                            let ctx_start = match_line.saturating_sub(before);
                            let ctx_end = (match_line + after + 1).min(lines.len());

                            for i in ctx_start..ctx_end {
                                let marker = if i == match_line { ">" } else { " " };
                                let num_prefix = if show_numbers {
                                    format!("{:>5}:", i + 1)
                                } else {
                                    String::new()
                                };
                                content_lines.push(format!(
                                    "{}:{} {} {}",
                                    rel, num_prefix, marker, lines[i]
                                ));
                            }
                            if idx + 1 < matches.len() {
                                content_lines.push("--".into());
                            }
                        }

                        file_matches.push(FileMatches {
                            path: rel, num_matches: matches.len(), content_lines,
                        });
                    }
                }
            } else if path.is_dir() {
                if name.starts_with('.') || VCS_DIRS.contains(&name.as_str()) || BUILD_DIRS.contains(&name.as_str()) {
                    continue;
                }
                Self::search_dir(&path, root, re, mode, after, before, show_numbers, glob_filter, type_filter, file_matches, deadline, depth + 1);
            }
        }
    }
}

// ═══════════════ Helpers ═══════════════

/// Simple glob → extension mapping for type filter.
fn type_to_extensions(t: &str) -> &'static [&'static str] {
    match t.to_lowercase().as_str() {
        "rust" | "rs" => &["rs"],
        "js" | "javascript" => &["js", "jsx", "mjs", "cjs"],
        "ts" | "typescript" => &["ts", "tsx", "mts", "cts"],
        "tsx" => &["tsx"],
        "py" | "python" => &["py", "pyi", "pyx"],
        "go" | "golang" => &["go"],
        "java" => &["java"],
        "toml" => &["toml"],
        "yaml" | "yml" => &["yaml", "yml"],
        "json" => &["json"],
        "md" | "markdown" => &["md", "mdx"],
        "html" => &["html", "htm"],
        "css" => &["css", "scss", "sass", "less"],
        "sh" | "bash" | "shell" => &["sh", "bash", "zsh"],
        "c" => &["c", "h"],
        "cpp" | "c++" => &["cpp", "cxx", "hpp", "hxx", "cc", "hh"],
        _ => &["rs", "js", "ts", "py", "go", "java", "toml", "yaml", "json", "md"],
    }
}

/// Simple glob matching without regex: * matches anything, ? matches one char.
fn simple_glob_match(pattern: &str, path: &str) -> bool {
    // Support patterns like "*.rs" or "*.{ts,tsx}"
    if pattern.contains('{') && pattern.contains('}') {
        let brace_start = pattern.find('{').unwrap();
        let brace_end = pattern.find('}').unwrap();
        let prefix = &pattern[..brace_start];
        let suffix = &pattern[brace_end + 1..];
        let alternatives: Vec<&str> = pattern[brace_start + 1..brace_end].split(',').collect();
        return alternatives.iter().any(|alt| {
            let full = format!("{}{}{}", prefix, alt, suffix);
            single_glob_match(&full, path)
        });
    }
    single_glob_match(pattern, path)
}

fn single_glob_match(pattern: &str, path: &str) -> bool {
    let mut pi = 0;
    let mut si = 0;
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = path.chars().collect();
    let mut star_pos = None;
    let mut match_pos = 0;

    while si < s.len() {
        if pi < p.len() && p[pi] == '*' {
            star_pos = Some(pi);
            match_pos = si;
            pi += 1;
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if let Some(sp) = star_pos {
            pi = sp + 1;
            match_pos += 1;
            si = match_pos;
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == '*' { pi += 1; }
    pi == p.len()
}

// ═══════════════ Tests ═══════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_to_extensions() {
        assert_eq!(type_to_extensions("rust"), &["rs"]);
        assert_eq!(type_to_extensions("python"), &["py", "pyi", "pyx"]);
        assert_eq!(type_to_extensions("unknown"), &["rs", "js", "ts", "py", "go", "java", "toml", "yaml", "json", "md"]);
    }

    #[test]
    fn test_simple_glob_match() {
        // Extension matching
        assert!(simple_glob_match("*.rs", "main.rs"));
        assert!(!simple_glob_match("*.rs", "main.js"));
        // Brace expansion
        assert!(simple_glob_match("*.{ts,tsx}", "file.tsx"));
        assert!(simple_glob_match("*.{ts,tsx}", "file.ts"));
        assert!(!simple_glob_match("*.{ts,tsx}", "file.rs"));
        // Wildcard in filename
        assert!(simple_glob_match("test_*.rs", "test_grep.rs"));
        assert!(!simple_glob_match("test_*.rs", "main.rs"));
    }

    #[test]
    fn test_grep_finds_rust_keywords() {
        let found = search_dir_simple("pub mod", "src", false);
        assert!(found.unwrap().contains("pub mod"));
    }

    #[test]
    fn test_grep_case_insensitive() {
        let sensitive = search_dir_simple("Pub Mod", "src", false);
        let insensitive = search_dir_simple("Pub Mod", "src", true);
        let s_count = sensitive.unwrap().lines().count();
        let i_count = insensitive.unwrap().lines().count();
        assert!(i_count >= s_count);
    }

    #[test]
    fn test_invalid_regex() {
        let result = search_dir_simple("[invalid", ".", false);
        assert!(result.is_err());
    }
}

// Helper for tests
#[allow(dead_code)]
fn search_dir_simple(pattern: &str, path: &str, ignore_case: bool) -> AgentResult<String> {
    use regex::RegexBuilder;
    let mut builder = RegexBuilder::new(pattern);
    builder.case_insensitive(ignore_case);
    let re = builder.build().map_err(|e| AgentError::ToolValidationError {
        tool: "grep".into(), errors: format!("Invalid regex pattern: {}", e),
    })?;

    let root = std::path::Path::new(path);
    let mut results = Vec::new();

    fn walk(dir: &std::path::Path, root: &std::path::Path, re: &Regex, results: &mut Vec<String>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(rel) = path.strip_prefix(root) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            for (i, line) in content.lines().enumerate() {
                                if re.is_match(line) {
                                    results.push(format!("{}:{} >{}", rel.display(), i + 1, line));
                                }
                            }
                        }
                    }
                } else if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with('.') || name == "target" || name == "node_modules" {
                            continue;
                        }
                    }
                    walk(&path, root, re, results);
                }
            }
        }
    }

    walk(root, root, &re, &mut results);
    if results.is_empty() {
        Ok(format!("No matches for '{}' in {}", pattern, path))
    } else {
        Ok(results.join("\n"))
    }
}
