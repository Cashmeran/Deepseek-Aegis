//! apply_patch — apply unified diff patches to files.
//! Handles multi-file, multi-hunk patches. Generator's primary editing tool.

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct ApplyPatchTool;

impl ToolMetadata for ApplyPatchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "apply_patch".into(),
            description: "Apply a unified diff patch to one or more files".into(),
            prompt: "Use apply_patch for multi-hunk, multi-file changes.\n\
                     - Accepts unified diff format (like git diff output)\n\
                     - Creates files that don't exist (new file mode)\n\
                     - Deletes files when the patch is a full deletion\n\
                     - Prefer file_edit for single replacements; use apply_patch for:\n\
                       coordinated multi-file changes, multi-hunk edits, file creation\n\
                     - The Generator uses this after planning coordinated changes".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "patch": {"type": "string", "description": "Unified diff patch text"},
                    "dry_run": {"type": "boolean", "description": "Preview changes without applying (default false)"}
                },
                "required": ["patch"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::High }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentUnsafe }
}

#[derive(Debug)]
struct Hunk {
    old_start: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug)]
enum HunkLine { Context(String), Add(String), Remove(String) }

#[derive(Debug)]
struct FilePatch {
    old_path: String,
    new_path: String,
    is_new: bool,
    is_delete: bool,
    hunks: Vec<Hunk>,
}

fn parse_patch(patch_text: &str) -> AgentResult<Vec<FilePatch>> {
    let mut patches = Vec::new();
    let mut current: Option<FilePatch> = None;
    let mut current_hunk: Option<Hunk> = None;

    for line in patch_text.lines() {
        if line.starts_with("diff --git ") {
            if let Some(f) = current.take() { patches.push(f); }
            current = Some(FilePatch { old_path: String::new(), new_path: String::new(), is_new: false, is_delete: false, hunks: Vec::new() });
        } else if line.starts_with("--- ") && current.is_some() {
            let path = line[4..].trim().to_string();
            if let Some(ref mut f) = current {
                f.old_path = path;
            }
        } else if line.starts_with("+++ ") {
            let path = line[4..].trim().to_string();
            if let Some(ref mut f) = current {
                f.new_path = path.clone();
                if f.old_path == "/dev/null" { f.is_new = true; }
                if path == "/dev/null" { f.is_delete = true; }
            }
        } else if line.starts_with("@@") {
            // @@ -old_start,old_count +new_start,new_count @@
            if let Some(h) = current_hunk.take() {
                if let Some(ref mut f) = current { f.hunks.push(h); }
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let old: Vec<usize> = parts[1].trim_start_matches('-').split(',').filter_map(|s| s.parse().ok()).collect();
                let _new: Vec<usize> = parts[2].trim_start_matches('+').split(',').filter_map(|s| s.parse().ok()).collect();
                if old.len() >= 1 {
                    current_hunk = Some(Hunk {
                        old_start: old[0].max(1),
                        lines: Vec::new(),
                    });
                }
            }
        } else if let Some(ref mut h) = current_hunk {
            match line.chars().next() {
                Some('+') => h.lines.push(HunkLine::Add(line[1..].to_string())),
                Some('-') => h.lines.push(HunkLine::Remove(line[1..].to_string())),
                Some(' ') => h.lines.push(HunkLine::Context(line[1..].to_string())),
                _ => {}
            }
        }
    }

    if let Some(h) = current_hunk.take() {
        if let Some(ref mut f) = current { f.hunks.push(h); }
    }
    if let Some(f) = current.take() {
        if !f.old_path.is_empty() { patches.push(f); }
    }

    Ok(patches)
}

fn apply_hunks(original: &str, hunks: &[Hunk]) -> AgentResult<String> {
    let lines: Vec<&str> = original.lines().collect();
    let mut result: Vec<String> = Vec::new();
    let mut line_idx = 0usize;

    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        // Copy lines before this hunk's start position
        let hunk_start = hunk.old_start.saturating_sub(1);
        if hunk_start < line_idx {
            return Err(AgentError::ToolExecutionError {
                tool: "apply_patch".into(),
                message: format!(
                    "Hunk {} overlaps with previous hunk (start={}, current position={})",
                    hunk_idx + 1, hunk.old_start, line_idx + 1
                ),
            });
        }
        while line_idx < hunk_start && line_idx < lines.len() {
            result.push(lines[line_idx].to_string());
            line_idx += 1;
        }

        // Verify hunk context/remove lines against file and apply
        let mut hunk_i = 0usize;
        while hunk_i < hunk.lines.len() {
            match &hunk.lines[hunk_i] {
                HunkLine::Context(expected) => {
                    if line_idx >= lines.len() || lines[line_idx] != expected {
                        return Err(AgentError::ToolExecutionError {
                            tool: "apply_patch".into(),
                            message: format!(
                                "Hunk {} context mismatch at line {}: expected '{}', got '{}'",
                                hunk_idx + 1, line_idx + 1,
                                expected,
                                if line_idx < lines.len() { lines[line_idx] } else { "(EOF)" }
                            ),
                        });
                    }
                    result.push(expected.clone());
                    line_idx += 1;
                }
                HunkLine::Remove(expected) => {
                    if line_idx >= lines.len() || lines[line_idx] != expected {
                        return Err(AgentError::ToolExecutionError {
                            tool: "apply_patch".into(),
                            message: format!(
                                "Hunk {} removal mismatch at line {}: expected '{}', got '{}'",
                                hunk_idx + 1, line_idx + 1,
                                expected,
                                if line_idx < lines.len() { lines[line_idx] } else { "(EOF)" }
                            ),
                        });
                    }
                    line_idx += 1;
                }
                HunkLine::Add(s) => {
                    result.push(s.clone());
                }
            }
            hunk_i += 1;
        }
    }

    // Copy remaining lines
    while line_idx < lines.len() {
        result.push(lines[line_idx].to_string());
        line_idx += 1;
    }

    Ok(result.join("\n"))
}

#[async_trait]
impl Tool for ApplyPatchTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let patch_text = tool_use.input.get("patch").and_then(|v| v.as_str()).unwrap_or("");
        let dry_run = tool_use.input.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

        if patch_text.is_empty() {
            return Err(AgentError::ToolValidationError { tool: "apply_patch".into(), errors: "patch is required".into() });
        }

        let mut patches = parse_patch(patch_text)?;
        if patches.is_empty() {
            return Err(AgentError::ToolExecutionError { tool: "apply_patch".into(), message: "No valid patches found".into() });
        }

        // Sort patches: new files first (create before reference), then edits, then deletes last
        patches.sort_by_key(|p| {
            if p.is_new { 0 }
            else if p.is_delete { 2 }
            else { 1 }
        });

        let mut report = String::from("## Patch Application\n\n");
        let mut modified = 0usize;
        let mut created = 0usize;
        let mut deleted = 0usize;
        let mut failed = Vec::new();
        let mut succeeded = Vec::new();

        for p in &patches {
            let result = if p.is_new {
                let new_content: String = p.hunks.iter()
                    .flat_map(|h| h.lines.iter().filter_map(|l| match l { HunkLine::Add(s) | HunkLine::Context(s) => Some(s.clone()), _ => None }))
                    .collect::<Vec<_>>().join("\n");

                if !dry_run {
                    if let Some(parent) = std::path::Path::new(&p.new_path).parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    match std::fs::write(&p.new_path, &new_content) {
                        Ok(_) => {
                            created += 1;
                            Ok(format!("NEW {} (+{} lines)", p.new_path, new_content.lines().count()))
                        }
                        Err(e) => Err(format!("NEW {} FAILED: {}", p.new_path, e)),
                    }
                } else {
                    Ok(format!("NEW {} (+{} lines) [dry-run]", p.new_path, new_content.lines().count()))
                }
            } else if p.is_delete {
                if !dry_run {
                    match std::fs::remove_file(&p.old_path) {
                        Ok(_) => { deleted += 1; Ok(format!("DELETE {}", p.old_path)) }
                        Err(e) => Err(format!("DELETE {} FAILED: {}", p.old_path, e)),
                    }
                } else {
                    Ok(format!("DELETE {} [dry-run]", p.old_path))
                }
            } else {
                let old_content = std::fs::read_to_string(&p.old_path).unwrap_or_default();
                match apply_hunks(&old_content, &p.hunks) {
                    Ok(new_content) => {
                        let diff_lines = new_content.lines().count() as isize - old_content.lines().count() as isize;
                        if !dry_run {
                            match std::fs::write(&p.new_path, &new_content) {
                                Ok(_) => {
                                    modified += 1;
                                    Ok(format!("EDIT {} ({:+} lines, {} hunks)", p.new_path, diff_lines, p.hunks.len()))
                                }
                                Err(e) => Err(format!("EDIT {} FAILED: {}", p.new_path, e)),
                            }
                        } else {
                            Ok(format!("EDIT {} ({:+} lines, {} hunks) [dry-run]", p.new_path, diff_lines, p.hunks.len()))
                        }
                    }
                    Err(e) => Err(format!("EDIT {} FAILED: {}", p.new_path, e)),
                }
            };

            match result {
                Ok(msg) => { report.push_str(&format!("  {}\n", msg)); succeeded.push(msg); }
                Err(msg) => { report.push_str(&format!("  {}\n", msg)); failed.push(msg); }
            }
        }

        report.push_str(&format!("\n{} modified, {} created, {} deleted, {} files total", modified, created, deleted, patches.len()));
        if !failed.is_empty() {
            report.push_str(&format!("\n{} FAILED: {}", failed.len(), failed.join("; ")));
        }
        if dry_run { report.push_str("\n(DRY RUN — no changes applied)"); }

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: !failed.is_empty(),
            content: vec![ContentBlock::Text { text: report }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_file_patch() {
        let patch = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
 }";
        let patches = parse_patch(patch).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].hunks.len(), 1);
    }

    #[test]
    fn test_apply_simple_hunk() {
        let original = "line1\nline2\nline3";
        // @@ -1,3 +1,3 @@ => start at old line 1, 3 lines of context
        let hunks = vec![Hunk { old_start: 1,
            lines: vec![
                HunkLine::Context("line1".into()),
                HunkLine::Remove("line2".into()),
                HunkLine::Add("modified".into()),
                HunkLine::Context("line3".into()),
            ]}];
        let result = apply_hunks(original, &hunks).unwrap();
        assert!(result.contains("modified"));
        assert!(!result.contains("line2"));
    }

    #[test]
    fn test_hunk_context_mismatch_fails() {
        let original = "line1\nline2\nline3";
        // Context("wrong") doesn't match the file at that position
        let hunks = vec![Hunk { old_start: 1,
            lines: vec![
                HunkLine::Context("wrong".into()),
                HunkLine::Remove("line2".into()),
                HunkLine::Add("line2b".into()),
            ]}];
        let result = apply_hunks(original, &hunks);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("context mismatch"));
    }
}
