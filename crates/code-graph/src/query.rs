use crate::store::GraphStore;
use crate::types::*;
use aegis_core::error::{AgentError, AgentResult};

/// Hard cap for tool result output (matches standard max result size).
const MAX_RESULT_CHARS: usize = 50_000;
/// How many items to show per category before summarizing.
const MAX_ITEMS_PER_CATEGORY: usize = 5;

/// Format a list of (edge, node) pairs into a compact summary.
fn summarize(items: &[(GraphEdge, GraphNode)], label: &str) -> Option<String> {
    if items.is_empty() { return None; }
    let names: Vec<&str> = items.iter().map(|(_, n)| n.name.as_str()).collect();
    let unique: Vec<&str> = {
        let mut seen = std::collections::HashSet::new();
        names.into_iter().filter(|n| seen.insert(*n)).collect()
    };
    let shown = unique.iter().take(MAX_ITEMS_PER_CATEGORY).copied().collect::<Vec<_>>();
    let total = unique.len();
    let line = if total <= MAX_ITEMS_PER_CATEGORY {
        shown.join(", ")
    } else {
        format!("{}, … ({} total)", shown.join(", "), total)
    };
    Some(format!("  {label}: {line}"))
}

/// Get architectural context for a file — compact Digest format.
/// Inspired by Nodex: returns a structured summary with hard size limits.
pub fn get_architectural_context(
    store: &dyn GraphStore,
    file_path: &str,
) -> AgentResult<String> {
    let normalized = file_path.replace('\\', "/");
    let file_nodes = store.get_file_nodes(&normalized)?;
    let file_node = file_nodes
        .iter()
        .find(|n| n.node_type == NodeType::File)
        .or_else(|| file_nodes.first())
        .ok_or_else(|| {
            AgentError::Internal(format!("File not indexed: {}", normalized))
        })?;

    let n = store.get_neighborhood(&file_node.id)?;
    let short_name = std::path::Path::new(&normalized)
        .file_name().map(|f| f.to_string_lossy()).unwrap_or_default();

    let _syms = file_nodes.iter().filter(|n| n.name != "__module__").count();
    let lines = file_nodes.iter()
        .map(|n| n.end_line)
        .max().unwrap_or(0) as usize;

    let mut parts: Vec<String> = vec![
        format!("{short_name}  {sym} symbols, ~{lines} lines, {sym} nodes",
                sym = file_nodes.len()),
    ];

    // Build categorized lists
    let by_edge = |et: EdgeType, dir: bool| -> Vec<(GraphEdge, GraphNode)> {
        let src = if dir { &n.outgoing } else { &n.incoming };
        src.iter().filter(|(e,_)| e.edge_type == et).cloned().collect()
    };

    for (et, label, dir) in [
        (EdgeType::Imports, "imports", true),
        (EdgeType::Imports, "imported by", false),
        (EdgeType::Calls, "calls", true),
        (EdgeType::Calls, "called by", false),
        (EdgeType::Inherits, "inherits", true),
    ] {
        if let Some(line) = summarize(&by_edge(et, dir), label) {
            parts.push(line);
        }
    }

    // Change risk assessment (Nodex-style)
    let imported_by = by_edge(EdgeType::Imports, false);
    let called_by = by_edge(EdgeType::Calls, false);
    let risk = if imported_by.len() + called_by.len() > 5 { "[WARN] high" }
        else if imported_by.len() + called_by.len() > 2 { "medium" }
        else { "low" };
    parts.push(format!("  change risk: {risk}"));

    // Warnings
    if lines > 500 { parts.push("  [WARN] >500 lines — large file".into()); }
    else if lines > 200 { parts.push("  [WARN] >200 lines".into()); }
    if file_nodes.len() > 20 { parts.push(format!("  [WARN] {} symbols — complex file", file_nodes.len())); }

    // Truncate to max result size
    let result = parts.join("\n");
    Ok(truncate(result, MAX_RESULT_CHARS, "...[truncated]"))
}

/// Impact map — what breaks if this symbol changes? (Nodex-style blast radius)
pub fn get_impact_map(
    store: &dyn GraphStore,
    symbol_name: &str,
) -> AgentResult<String> {
    // Search for matching nodes
    let matches = store.search_nodes(symbol_name, None, 20)?;
    if matches.is_empty() {
        return Ok(format!("No symbols found matching '{symbol_name}'"));
    }

    let mut parts = vec![format!("Impact map for '{}':", symbol_name)];
    for node in matches.iter().take(5) {
        let n = store.get_neighborhood(&node.id)?;
        let incoming: Vec<_> = n.incoming.iter()
            .filter(|(e,_)| e.edge_type == EdgeType::Calls || e.edge_type == EdgeType::Imports)
            .collect();
        let incoming_count = incoming.len();
        let who: Vec<String> = incoming.iter()
            .take(MAX_ITEMS_PER_CATEGORY)
            .map(|(_, n)| format!("{} ({})", n.name, n.file_path))
            .collect();
        let risk = if incoming_count > 5 { "[WARN] high" }
            else if incoming_count > 2 { "medium" }
            else { "low" };

        let line = if incoming_count == 0 {
            format!("  {} ({}:{}): no dependents", node.name, node.file_path, node.start_line)
        } else if incoming_count <= MAX_ITEMS_PER_CATEGORY {
            format!("  {} ({}:{}): {} dependents [{}] → {}", node.name, node.file_path, node.start_line, incoming_count, risk, who.join(", "))
        } else {
            format!("  {} ({}:{}): {} dependents [{}] → {} … ({} total)", node.name, node.file_path, node.start_line, incoming_count, risk, who.join(", "), incoming_count)
        };
        parts.push(line);
    }

    if matches.len() > 5 {
        parts.push(format!("  ... and {} more matches", matches.len() - 5));
    }
    let result = parts.join("\n");
    Ok(truncate(result, MAX_RESULT_CHARS, "...[truncated]"))
}

fn truncate(s: String, max: usize, suffix: &str) -> String {
    if s.chars().count() <= max { return s; }
    let truncated: String = s.chars().take(max.saturating_sub(suffix.len())).collect();
    format!("{truncated}{suffix}")
}

/// Detect project root for a file by walking up to find a manifest file.
/// Recognizes: Cargo.toml (Rust), package.json (JS/TS), go.mod (Go), pyproject.toml (Python).
pub fn detect_project_root(file_path: &str) -> Option<String> {
    let path = std::path::Path::new(file_path);
    let mut current = path.parent();
    while let Some(dir) = current {
        for manifest in &["Cargo.toml", "package.json", "go.mod", "pyproject.toml", "CMakeLists.txt"] {
            if dir.join(manifest).exists() {
                return Some(dir.to_string_lossy().replace('\\', "/"));
            }
        }
        current = dir.parent();
    }
    // Fallback: use the parent directory of the file
    path.parent().map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// Generate project-aware codebase overview.
/// Groups files by project (detected from manifest files), shows per-project stats.
pub fn get_codebase_overview(store: &dyn GraphStore) -> AgentResult<String> {
    let files = store.list_files()?;
    if files.is_empty() { return Ok(String::new()); }

    let total_nodes = store.node_count()?;
    let total_edges = store.edge_count()?;
    let mut parts = vec![format!(
        "## Codebase Overview\n{} files, {} symbols, {} relationships indexed.",
        files.len(), total_nodes, total_edges
    )];

    // Group files by project
    let mut projects: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for file in &files {
        let path = file.to_string_lossy().replace('\\', "/");
        let root = detect_project_root(&path).unwrap_or_else(|| ".".into());
        projects.entry(root).or_default().push(path);
    }

    // Per-project summary
    if projects.len() > 1 {
        parts.push(format!("{} projects detected:", projects.len()));
        let mut sorted_projects: Vec<_> = projects.iter().collect();
        sorted_projects.sort_by_key(|(_, files)| std::cmp::Reverse(files.len()));
        for (root, proj_files) in sorted_projects.iter().take(8) {
            let name = if root.as_str() == "." { "root".to_string() } else {
                std::path::Path::new(root).file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.to_string())
            };
            parts.push(format!("  {name}/ — {} files", proj_files.len()));
        }
        if projects.len() > 8 {
            parts.push(format!("  ... and {} more projects", projects.len() - 8));
        }
    } else {
        // Single project: show module structure
        let mut modules: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for file in &files {
            let path = file.to_string_lossy().replace('\\', "/");
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let mod_name = parent.to_string_lossy().replace('\\', "/");
                *modules.entry(mod_name).or_default() += 1;
            }
        }
        let top_modules: Vec<_> = modules.iter()
            .filter(|(_, c)| **c >= 2)
            .take(8)
            .collect();
        if !top_modules.is_empty() {
            parts.push("Module structure:".into());
            for (m, c) in &top_modules {
                parts.push(format!("  {m}/ — {c} files"));
            }
        }
    }

    Ok(truncate(parts.join("\n"), 3000, "..."))
}

