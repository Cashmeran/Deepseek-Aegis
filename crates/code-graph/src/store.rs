use crate::types::*;
use aegis_core::error::{AgentError, AgentResult};
use rusqlite::params;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub trait GraphStore: Send + Sync {
    fn open(path: &Path) -> AgentResult<Self> where Self: Sized;
    fn upsert_file_nodes(&self, file_path: &Path, nodes: &[GraphNode], edges: &[GraphEdge]) -> AgentResult<()>;
    fn remove_file(&self, file_path: &Path) -> AgentResult<()>;
    fn get_node(&self, id: &NodeId) -> AgentResult<Option<GraphNode>>;
    fn get_neighborhood(&self, node_id: &NodeId) -> AgentResult<NeighborhoodResult>;
    fn bfs_traverse(&self, start_id: &NodeId, max_depth: usize) -> AgentResult<Vec<BfsNode>>;
    fn find_callers(&self, node_id: &NodeId, limit: usize) -> AgentResult<Vec<(GraphEdge, GraphNode)>>;
    fn find_callees(&self, node_id: &NodeId, limit: usize) -> AgentResult<Vec<(GraphEdge, GraphNode)>>;
    fn get_file_hash(&self, path: &Path) -> AgentResult<Option<String>>;
    fn list_files(&self) -> AgentResult<Vec<PathBuf>>;
    fn node_count(&self) -> AgentResult<usize>;
    fn edge_count(&self) -> AgentResult<usize>;
    fn get_file_nodes(&self, file_path: &str) -> AgentResult<Vec<GraphNode>>;
    fn search_nodes(&self, name_pattern: &str, node_type: Option<NodeType>, limit: usize) -> AgentResult<Vec<GraphNode>>;
    /// Resolve cross-file call edges by matching target_name to node names.
    /// Default: no-op. Override in SqliteGraphStore.
    fn resolve_cross_file_calls(&self) -> AgentResult<usize> { Ok(0) }
}

pub struct SqliteGraphStore {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteGraphStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.conn.lock().expect("SqliteGraphStore Mutex poisoned")
    }

    fn init_schema(c: &rusqlite::Connection) -> AgentResult<()> {
        c.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -8000;

             CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY NOT NULL,
                node_type INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                name TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                start_col INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_col INTEGER NOT NULL,
                visibility INTEGER NOT NULL DEFAULT 2,
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );

             CREATE TABLE IF NOT EXISTS edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
                target_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
                edge_type INTEGER NOT NULL,
                weight REAL NOT NULL DEFAULT 0.5,
                target_name TEXT NOT NULL DEFAULT '',
                UNIQUE(source_id, target_id, edge_type)
             );

             CREATE TABLE IF NOT EXISTS file_hashes (
                path TEXT PRIMARY KEY NOT NULL,
                hash TEXT NOT NULL,
                language TEXT NOT NULL DEFAULT '',
                node_count INTEGER NOT NULL DEFAULT 0,
                edge_count INTEGER NOT NULL DEFAULT 0,
                parse_error_count INTEGER NOT NULL DEFAULT 0,
                indexed_at TEXT NOT NULL DEFAULT (datetime('now'))
             );

             CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(file_path);
             CREATE INDEX IF NOT EXISTS idx_nodes_type ON nodes(node_type);
             CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
             CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
             CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
             CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);
             CREATE INDEX IF NOT EXISTS idx_edges_source_target ON edges(source_id, target_id);
             CREATE INDEX IF NOT EXISTS idx_file_hashes_path ON file_hashes(path);",
        )
        .map_err(|e| AgentError::Internal(format!("Schema init failed: {}", e)))?;
        Ok(())
    }

    fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<GraphNode> {
        Ok(GraphNode {
            id: row.get(0)?,
            node_type: NodeType::from_u8(row.get::<_, i32>(1)? as u8).unwrap_or(NodeType::Function),
            file_path: row.get(2)?,
            name: row.get(3)?,
            start_line: row.get::<_, i32>(4)? as u32,
            start_col: row.get::<_, i32>(5)? as u32,
            end_line: row.get::<_, i32>(6)? as u32,
            end_col: row.get::<_, i32>(7)? as u32,
            visibility: match row.get::<_, i32>(8)? { 0 => Visibility::Public, 1 => Visibility::Crate, _ => Visibility::Private },
            metadata: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
        })
    }
}

impl GraphStore for SqliteGraphStore {
    fn open(path: &Path) -> AgentResult<Self> {
        let c = rusqlite::Connection::open(path)
            .map_err(|e| AgentError::Internal(format!("Cannot open DB: {}", e)))?;
        Self::init_schema(&c)?;
        c.execute_batch("PRAGMA foreign_keys = OFF;").ok();
        // Migration: add target_name column for cross-file call resolution
        c.execute_batch(
            "ALTER TABLE edges ADD COLUMN target_name TEXT NOT NULL DEFAULT '';"
        ).ok(); // ignore error if column already exists
        Ok(Self { conn: Mutex::new(c) })
    }

    fn upsert_file_nodes(&self, file_path: &Path, nodes: &[GraphNode], edges: &[GraphEdge]) -> AgentResult<()> {
        let c = self.lock();
        let path_str = file_path.to_string_lossy().replace('\\', "/");
        // Disable FK on the connection BEFORE starting transaction
        c.execute_batch("PRAGMA foreign_keys = OFF;").ok();
        let tx = c.unchecked_transaction().map_err(|e| AgentError::Internal(format!("Tx: {}", e)))?;

        tx.execute("DELETE FROM nodes WHERE file_path = ?1", [&path_str])
            .map_err(|e| AgentError::Internal(format!("Delete: {}", e)))?;

        for chunk in nodes.chunks(500) {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO nodes (id, node_type, file_path, name, start_line, start_col, end_line, end_col, visibility, metadata) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
            ).map_err(|e| AgentError::Internal(format!("Prep node: {}", e)))?;
            for node in chunk {
                stmt.execute(params![node.id, node.node_type.to_u8(), node.file_path, node.name, node.start_line, node.start_col, node.end_line, node.end_col, node.visibility as u8, node.metadata.to_string()])
                    .map_err(|e| AgentError::Internal(format!("Ins node: {}", e)))?;
            }
        }
        for chunk in edges.chunks(500) {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO edges (source_id, target_id, edge_type, weight, target_name) VALUES (?1,?2,?3,?4,?5)"
            ).map_err(|e| AgentError::Internal(format!("Prep edge: {}", e)))?;
            for edge in chunk {
                stmt.execute(params![edge.source_id, edge.target_id, edge.edge_type.to_u8(), edge.weight, edge.target_name])
                    .map_err(|e| AgentError::Internal(format!("Ins edge: {}", e)))?;
            }
        }
        let lang = LanguageId::from_extension(&path_str).map(|l| format!("{:?}", l)).unwrap_or_default();
        tx.execute("INSERT OR REPLACE INTO file_hashes (path, hash, language, node_count, edge_count, indexed_at) VALUES (?1,?2,?3,?4,?5,datetime('now'))",
            params![path_str, "", lang, nodes.len(), edges.len()])
            .map_err(|e| AgentError::Internal(format!("Hash: {}", e)))?;
        tx.commit().map_err(|e| AgentError::Internal(format!("Commit: {}", e)))?;
        Ok(())
    }

    fn remove_file(&self, file_path: &Path) -> AgentResult<()> {
        let c = self.lock();
        let path_str = file_path.to_string_lossy().replace('\\', "/");
        c.execute("DELETE FROM file_hashes WHERE path = ?1", [&path_str]).ok();
        c.execute("DELETE FROM nodes WHERE file_path = ?1", [&path_str])
            .map_err(|e| AgentError::Internal(format!("Del nodes: {}", e)))?;
        Ok(())
    }

    fn get_node(&self, id: &NodeId) -> AgentResult<Option<GraphNode>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached(
            "SELECT id,node_type,file_path,name,start_line,start_col,end_line,end_col,visibility,metadata FROM nodes WHERE id=?1"
        ).map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
        match stmt.query_row([id], Self::row_to_node) {
            Ok(node) => Ok(Some(node)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Internal(format!("Query: {}", e))),
        }
    }

    fn get_neighborhood(&self, node_id: &NodeId) -> AgentResult<NeighborhoodResult> {
        let center = self.get_node(node_id)?.ok_or_else(|| AgentError::Internal(format!("Node not found: {}", node_id)))?;

        // Collect edge rows under lock, then release before resolving nodes.
        // get_node() also acquires the lock, and std::sync::Mutex is NOT reentrant.
        let irows: Vec<(String,String,i32,f32,String)>;
        let orows: Vec<(String,String,i32,f32,String)>;
        {
            let c = self.lock();
            let mut istmt = c.prepare_cached(
                "SELECT source_id,target_id,edge_type,weight,target_name FROM edges WHERE target_id=?1 LIMIT 100"
            ).map_err(|e| AgentError::Internal(format!("Prep in: {}", e)))?;
            irows = istmt.query_map([node_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))
                .map_err(|e| AgentError::Internal(format!("Q in: {}", e)))?.filter_map(|r| r.ok()).collect();
            let mut ostmt = c.prepare_cached(
                "SELECT source_id,target_id,edge_type,weight,target_name FROM edges WHERE source_id=?1 LIMIT 100"
            ).map_err(|e| AgentError::Internal(format!("Prep out: {}", e)))?;
            orows = ostmt.query_map([node_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))
                .map_err(|e| AgentError::Internal(format!("Q out: {}", e)))?.filter_map(|r| r.ok()).collect();
        } // lock released here; get_node() calls below can now acquire it safely

        let mut incoming = Vec::new();
        for (src, tgt, et, wt, tn) in irows {
            if let Ok(Some(node)) = self.get_node(&src) {
                incoming.push((GraphEdge { source_id: src, target_id: tgt, edge_type: EdgeType::from_u8(et as u8).unwrap_or(EdgeType::Calls), weight: wt, target_name: tn }, node));
            }
        }

        let mut outgoing = Vec::new();
        for (src, tgt, et, wt, tn) in orows {
            if let Ok(Some(node)) = self.get_node(&tgt) {
                outgoing.push((GraphEdge { source_id: src, target_id: tgt, edge_type: EdgeType::from_u8(et as u8).unwrap_or(EdgeType::Calls), weight: wt, target_name: tn }, node));
            }
        }
        Ok(NeighborhoodResult { center, incoming, outgoing })
    }

    fn bfs_traverse(&self, start_id: &NodeId, max_depth: usize) -> AgentResult<Vec<BfsNode>> {
        let start_node = self.get_node(start_id)?.ok_or_else(|| AgentError::Internal("Start node not found".into()))?;
        if max_depth == 0 {
            return Ok(vec![BfsNode { node: start_node, depth: 0, incoming_from: None }]);
        }
        let all_edges: Vec<(NodeId,NodeId)> = {
            let c = self.lock();
            let mut estmt = c.prepare("SELECT source_id, target_id FROM edges").map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
            estmt.query_map([], |r| Ok((r.get(0)?,r.get(1)?)))
                .map_err(|e| AgentError::Internal(format!("Q: {}", e)))?.filter_map(|r| r.ok()).collect()
        };

        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut queue: VecDeque<(GraphNode, usize, Option<String>)> = VecDeque::new();
        let mut result = Vec::new();
        visited.insert(start_id.clone());
        queue.push_back((start_node, 0, None));
        while let Some((node, depth, incoming)) = queue.pop_front() {
            result.push(BfsNode { node: node.clone(), depth, incoming_from: incoming });
            if depth >= max_depth { continue; }
            for (src, tgt) in &all_edges {
                if src == &node.id && !visited.contains(tgt) {
                    visited.insert(tgt.clone());
                    if let Ok(Some(next)) = self.get_node(tgt) {
                        queue.push_back((next, depth + 1, Some(node.id.clone())));
                    }
                }
            }
        }
        Ok(result)
    }

    fn find_callers(&self, node_id: &NodeId, limit: usize) -> AgentResult<Vec<(GraphEdge, GraphNode)>> {
        let rows: Vec<(String,String,i32,f32,String)> = {
            let c = self.lock();
            let mut stmt = c.prepare_cached(
                "SELECT source_id,target_id,edge_type,weight,target_name FROM edges WHERE target_id=?1 AND edge_type=?2 ORDER BY weight DESC LIMIT ?3"
            ).map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
            stmt.query_map(params![node_id, EdgeType::Calls.to_u8(), limit.min(200) as i64], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))
                .map_err(|e| AgentError::Internal(format!("Q: {}", e)))?.filter_map(|r| r.ok()).collect()
        };
        let mut result = Vec::new();
        for (src, tgt, et, wt, tn) in rows {
            if let Ok(Some(node)) = self.get_node(&src) {
                result.push((GraphEdge { source_id: src, target_id: tgt, edge_type: EdgeType::from_u8(et as u8).unwrap_or(EdgeType::Calls), weight: wt, target_name: tn }, node));
            }
        }
        Ok(result)
    }

    fn find_callees(&self, node_id: &NodeId, limit: usize) -> AgentResult<Vec<(GraphEdge, GraphNode)>> {
        let rows: Vec<(String,String,i32,f32,String)> = {
            let c = self.lock();
            let mut stmt = c.prepare_cached(
                "SELECT source_id,target_id,edge_type,weight,target_name FROM edges WHERE source_id=?1 AND edge_type=?2 ORDER BY weight DESC LIMIT ?3"
            ).map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
            stmt.query_map(params![node_id, EdgeType::Calls.to_u8(), limit.min(200) as i64], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))
                .map_err(|e| AgentError::Internal(format!("Q: {}", e)))?.filter_map(|r| r.ok()).collect()
        };
        let mut result = Vec::new();
        for (src, tgt, et, wt, tn) in rows {
            if let Ok(Some(node)) = self.get_node(&tgt) {
                result.push((GraphEdge { source_id: src, target_id: tgt, edge_type: EdgeType::from_u8(et as u8).unwrap_or(EdgeType::Calls), weight: wt, target_name: tn }, node));
            }
        }
        Ok(result)
    }

    fn get_file_hash(&self, path: &Path) -> AgentResult<Option<String>> {
        let c = self.lock();
        let path_str = path.to_string_lossy().replace('\\', "/");
        let mut stmt = c.prepare_cached("SELECT hash FROM file_hashes WHERE path=?1")
            .map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
        match stmt.query_row([&path_str], |r| r.get(0)) {
            Ok(h) => Ok(Some(h)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Internal(format!("Q: {}", e))),
        }
    }

    fn list_files(&self) -> AgentResult<Vec<PathBuf>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached("SELECT path FROM file_hashes ORDER BY path ASC")
            .map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
        let rows = stmt.query_map([], |r| Ok(PathBuf::from(r.get::<_,String>(0)?)))
            .map_err(|e| AgentError::Internal(format!("Q: {}", e)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn node_count(&self) -> AgentResult<usize> {
        let c = self.lock();
        c.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get::<_,i64>(0))
            .map(|n| n as usize).map_err(|e| AgentError::Internal(format!("Count: {}", e)))
    }

    fn edge_count(&self) -> AgentResult<usize> {
        let c = self.lock();
        c.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get::<_,i64>(0))
            .map(|n| n as usize).map_err(|e| AgentError::Internal(format!("Count: {}", e)))
    }

    fn get_file_nodes(&self, file_path: &str) -> AgentResult<Vec<GraphNode>> {
        let c = self.lock();
        let path_str = file_path.replace('\\', "/");
        let mut stmt = c.prepare_cached(
            "SELECT id,node_type,file_path,name,start_line,start_col,end_line,end_col,visibility,metadata FROM nodes WHERE file_path=?1 ORDER BY start_line"
        ).map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
        let rows = stmt.query_map([&path_str], Self::row_to_node)
            .map_err(|e| AgentError::Internal(format!("Q: {}", e)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn search_nodes(&self, name_pattern: &str, node_type: Option<NodeType>, limit: usize) -> AgentResult<Vec<GraphNode>> {
        let c = self.lock();
        let pattern = format!("%{}%", name_pattern);
        let limit = limit.min(50) as i64;
        if let Some(nt) = node_type {
            let sql = "SELECT id,node_type,file_path,name,start_line,start_col,end_line,end_col,visibility,metadata FROM nodes WHERE name LIKE ?1 AND node_type=?2 ORDER BY name LIMIT ?3";
            let mut stmt = c.prepare_cached(sql).map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
            let rows = stmt.query_map(params![pattern, nt.to_u8(), limit], Self::row_to_node)
                .map_err(|e| AgentError::Internal(format!("Q: {}", e)))?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        } else {
            let sql = "SELECT id,node_type,file_path,name,start_line,start_col,end_line,end_col,visibility,metadata FROM nodes WHERE name LIKE ?1 ORDER BY name LIMIT ?2";
            let mut stmt = c.prepare_cached(sql).map_err(|e| AgentError::Internal(format!("Prep: {}", e)))?;
            let rows = stmt.query_map(params![pattern, limit], Self::row_to_node)
                .map_err(|e| AgentError::Internal(format!("Q: {}", e)))?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        }
    }

    fn resolve_cross_file_calls(&self) -> AgentResult<usize> {
        use crate::types::{EdgeType, NodeType};
        let c = self.lock();

        // Collect all dangling Call edges (target doesn't exist in nodes)
        let mut stmt = c.prepare(
            "SELECT e.rowid, e.source_id, e.target_name, n2.file_path as src_file
             FROM edges e
             LEFT JOIN nodes n2 ON e.source_id = n2.id
             WHERE e.edge_type = ?1
               AND e.target_name != ''
               AND e.target_id NOT IN (SELECT id FROM nodes)"
        ).map_err(|e| AgentError::Internal(format!("prep: {e}")))?;

        struct DanglingEdge {
            rowid: i64,
            target_name: String,
            src_file: String,
        }

        let dangling: Vec<DanglingEdge> = stmt.query_map(
            params![EdgeType::Calls.to_u8()],
            |row| Ok(DanglingEdge {
                rowid: row.get(0)?,
                // source_id isn't used directly; we match by target_name only
                target_name: row.get::<_, String>(2)?,
                src_file: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            })
        ).map_err(|e| AgentError::Internal(format!("q: {e}")))?
         .filter_map(|r| r.ok())
         .collect();

        let mut resolved = 0usize;
        for edge in &dangling {
            // Find a definition node matching the target_name (Function or Struct)
            let mut find_stmt = c.prepare(
                "SELECT id, file_path FROM nodes WHERE name = ?1 AND node_type IN (?2, ?3) ORDER BY file_path LIMIT 1"
            ).map_err(|e| AgentError::Internal(format!("prep find: {e}")))?;

            let found: Option<(String, String)> = find_stmt.query_row(
                params![edge.target_name, NodeType::Function.to_u8(), NodeType::Struct.to_u8()],
                |row| Ok((row.get(0)?, row.get(1)?))
            ).ok();

            if let Some((def_id, def_file)) = found {
                c.execute(
                    "UPDATE edges SET target_id = ?1 WHERE rowid = ?2",
                    params![def_id, edge.rowid],
                ).map_err(|e| AgentError::Internal(format!("update: {e}")))?;
                resolved += 1;
                tracing::debug!(
                    "Resolved edge: {} -> {}::{}",
                    edge.src_file, def_file, edge.target_name
                );
            }
        }

        tracing::info!("Cross-file resolution: {} dangling edges, {} resolved", dangling.len(), resolved);
        Ok(resolved)
    }
}
