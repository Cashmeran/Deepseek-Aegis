//! Safe file operations — path traversal protection, known-bad filename patterns

use crate::error::{AgentError, AgentResult};

/// Check for path traversal (.., ~, / absolute paths outside workspace)
pub fn check_path_safety(path: &str, _workspace_root: &str) -> AgentResult<()> {
    let normalized = path.replace('\\', "/");

    if normalized.contains("..") {
        return Err(AgentError::PathTraversalBlocked {
            path: normalized,
            resolved: "Path traversal '..' not allowed".into(),
        });
    }

    if normalized.starts_with('/') || normalized.starts_with('~') {
        return Err(AgentError::PathTraversalBlocked {
            path: normalized,
            resolved: "Absolute paths not allowed".into(),
        });
    }

    // Unicode homoglyph attacks
    if normalized.contains('\u{2024}') || normalized.contains('\u{2215}') {
        return Err(AgentError::PathTraversalBlocked {
            path: normalized,
            resolved: "Unicode homoglyph path attack detected".into(),
        });
    }

    Ok(())
}

/// Check if a file is in the workspace
pub fn is_in_workspace(path: &str, workspace: &str) -> bool {
    let canonical = path.replace('\\', "/");
    let ws = workspace.replace('\\', "/");
    canonical.starts_with(&ws)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_traversal_blocked() {
        assert!(check_path_safety("../etc/passwd", "/workspace").is_err());
        assert!(check_path_safety("..\\..\\windows", "/workspace").is_err());
    }

    #[test]
    fn test_absolute_path_blocked() {
        assert!(check_path_safety("/etc/passwd", "/workspace").is_err());
    }

    #[test]
    fn test_safe_path_allowed() {
        assert!(check_path_safety("src/main.rs", "/workspace").is_ok());
        assert!(check_path_safety("a/b/c/d.rs", "/workspace").is_ok());
    }
}
