//! Code graph query for the desktop visualization panel.
//! Returns React Flow-compatible { nodes, edges } from .aegis/code-graph/graph.db.

use std::path::PathBuf;

use aegis_code_graph::{GraphStore, SqliteGraphStore};

#[tauri::command]
pub fn get_code_graph(cwd: Option<String>) -> Result<serde_json::Value, String> {
    let cwd = cwd.unwrap_or_else(|| ".".into());
    let db_path = PathBuf::from(&cwd).join(".aegis").join("graph.db");

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    log::info!("get_code_graph: cwd={cwd}, db={}", db_path.display());
    if db_path.exists() {
        let store = SqliteGraphStore::open(&db_path)
            .map_err(|e| format!("Failed to open graph DB: {e}"))?;

        if let Ok(viz_nodes) = store.get_all_nodes_for_viz() {
            log::info!("get_code_graph: {} viz nodes", viz_nodes.len());
            for n in viz_nodes {
                let name = std::path::Path::new(&n.path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| n.path.clone());
                nodes.push(serde_json::json!({
                    "id": n.id,
                    "data": {
                        "label": name,
                        "path": n.path,
                        "language": n.language,
                        "nodeCount": n.node_count,
                    },
                    "position": { "x": 0, "y": 0 },
                }));
            }
        }

        if let Ok(viz_edges) = store.get_all_edges_for_viz() {
            for (i, e) in viz_edges.iter().enumerate() {
                edges.push(serde_json::json!({
                    "id": format!("e{i}"),
                    "source": e.source,
                    "target": e.target,
                    "data": { "weight": e.weight },
                }));
            }
        }
    }

    Ok(serde_json::json!({ "nodes": nodes, "edges": edges }))
}
