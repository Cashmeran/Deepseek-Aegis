use crate::schema::MEMORY_DDL;
use crate::types::*;
use aegis_core::error::{AgentError, AgentResult};
use rusqlite::params;
use sha2::Digest;
use std::path::Path;
use std::sync::Mutex;

/// Causal memory persistence trait — consumed by AgentLoop and Consolidator.
pub trait MemoryStore: Send + Sync {
    fn open(path: &Path) -> AgentResult<Self> where Self: Sized;

    // Episode lifecycle
    fn create_episode(&self, session_id: &str, user_request: &str) -> AgentResult<MemoryNodeId>;
    fn label_episode(&self, episode_id: &MemoryNodeId, outcome: EpisodeOutcome, agent_response: &str, error_signature: Option<&str>) -> AgentResult<()>;
    fn get_session_episodes(&self, session_id: &str) -> AgentResult<Vec<Episode>>;
    fn get_episode(&self, id: &MemoryNodeId) -> AgentResult<Option<Episode>>;

    // CRUD
    fn record_bug(&self, bug: &Bug) -> AgentResult<MemoryNodeId>;
    fn record_fix(&self, fix: &Fix, bug_id: &MemoryNodeId) -> AgentResult<MemoryNodeId>;
    fn record_fix_no_bug(&self, fix: &Fix) -> AgentResult<MemoryNodeId>;
    fn record_root_cause(&self, rc: &RootCause, bug_ids: &[MemoryNodeId]) -> AgentResult<MemoryNodeId>;
    fn upsert_insight(&self, insight: &Insight, episode_ids: &[MemoryNodeId]) -> AgentResult<MemoryNodeId>;
    fn get_node(&self, id: &MemoryNodeId) -> AgentResult<Option<MemoryNode>>;
    fn get_bug(&self, id: &MemoryNodeId) -> AgentResult<Option<Bug>>;
    fn get_insight(&self, id: &MemoryNodeId) -> AgentResult<Option<Insight>>;

    // Embeddings
    fn store_embedding(&self, node_id: &MemoryNodeId, vector: &[f32]) -> AgentResult<()>;
    fn get_embedding(&self, node_id: &MemoryNodeId) -> AgentResult<Option<Vec<f32>>>;
    fn get_all_embeddings(&self, node_type: Option<MemoryNodeType>) -> AgentResult<Vec<(MemoryNodeId, Vec<f32>)>>;

    // Graph
    fn get_neighborhood(&self, node_id: &MemoryNodeId, max_depth: usize) -> AgentResult<Vec<(MemoryNodeId, MemoryEdgeType, f32)>>;
    fn find_similar_bugs(&self, embedding: &[f32], limit: usize) -> AgentResult<Vec<(Bug, f32)>>;
    fn find_related_insights(&self, bug_id: &MemoryNodeId) -> AgentResult<Vec<Insight>>;
    fn get_recent_insights(&self, limit: usize) -> AgentResult<Vec<Insight>>;

    // Query helpers
    fn count_similar_patterns(&self, episode: &Episode) -> AgentResult<u32>;
    fn count_cross_session_occurrences(&self, episode: &Episode) -> AgentResult<u32>;
    fn find_bugs_by_signature(&self, error_sig: &str) -> AgentResult<Vec<Bug>>;

    // Gate & consolidation
    fn get_pending_consolidation_episodes(&self, min_sessions: u32, min_age_hours: u32) -> AgentResult<Vec<Episode>>;
    fn get_consolidation_state(&self) -> AgentResult<ConsolidationState>;
    fn set_consolidation_state(&self, state: ConsolidationState) -> AgentResult<()>;
    fn prune_insights(&self, min_utility: f32, max_age_days: u32) -> AgentResult<u32>;
    /// Delete old episodes with no outgoing edges (cleanup)
    fn prune_episodes(&self, max_age_days: u32) -> AgentResult<u32>;
    fn find_supporting_episodes(&self, node_id: &MemoryNodeId) -> AgentResult<Vec<MemoryNodeId>>;
    fn record_correction(&self, episode_id: &MemoryNodeId, original: &str, correction: &str) -> AgentResult<()>;

    // Stats
    fn node_count(&self) -> AgentResult<usize>;
    fn edge_count(&self) -> AgentResult<usize>;
    fn total_episodes(&self) -> AgentResult<usize>;
}

// ═══════════════ SQLite impl ═══════════════

pub struct SqliteMemoryStore {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteMemoryStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.conn.lock().expect("SqliteMemoryStore Mutex poisoned")
    }

    fn insert_node(&self, c: &rusqlite::Connection, id: &str, node_type: MemoryNodeType, content: &str) -> AgentResult<()> {
        let hash = format!("{:x}", sha2::Sha256::new().chain_update(content.as_bytes()).finalize());
        c.execute(
            "INSERT INTO memory_nodes (id, node_type, content, content_hash) VALUES (?1,?2,?3,?4)",
            params![id, node_type.to_u8(), content, hash],
        ).map_err(|e| AgentError::Internal(format!("insert node: {}", e)))?;
        Ok(())
    }

    fn get_episode_from_row(row: &rusqlite::Row) -> rusqlite::Result<Episode> {
        let content: String = row.get(0)?;
        serde_json::from_str(&content).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
    }

    fn get_bug_from_row(row: &rusqlite::Row) -> rusqlite::Result<Bug> {
        let content: String = row.get(0)?;
        serde_json::from_str(&content).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
    }

    fn get_insight_from_row(row: &rusqlite::Row) -> rusqlite::Result<Insight> {
        let content: String = row.get(0)?;
        serde_json::from_str(&content).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
    }

    pub fn archived_count(&self) -> usize {
        let c = self.lock();
        c.query_row(
            "SELECT COUNT(*) FROM episodes WHERE created_at < datetime('now', '-30 days')",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as usize)
        .unwrap_or(0)
    }
}

impl MemoryStore for SqliteMemoryStore {
    fn open(path: &Path) -> AgentResult<Self> {
        let c = rusqlite::Connection::open(path)
            .map_err(|e| AgentError::Internal(format!("Cannot open DB: {}", e)))?;
        c.execute_batch(MEMORY_DDL)
            .map_err(|e| AgentError::Internal(format!("Schema init: {}", e)))?;
        Ok(Self { conn: Mutex::new(c) })
    }

    fn create_episode(&self, session_id: &str, user_request: &str) -> AgentResult<MemoryNodeId> {
        let c = self.lock();
        let ts = chrono::Utc::now().timestamp();
        let id = make_memory_id(user_request, "Episode", ts);
        let ep = Episode {
            id: id.clone(), session_id: session_id.into(), user_request: user_request.into(),
            agent_response: String::new(), outcome: EpisodeOutcome::Unknown,
            error_signature: None, tools_used: vec![], files_modified: vec![],
            token_usage: 0, duration_ms: 0, created_at: chrono::Utc::now(),
            metadata: serde_json::json!({}),
        };
        let json = serde_json::to_string(&ep).map_err(|e| AgentError::Internal(format!("serialize: {}", e)))?;
        self.insert_node(&c, &id, MemoryNodeType::Episode, &json)?;
        c.execute("INSERT INTO episodes (id, session_id, outcome) VALUES (?1,?2,?3)",
            params![id, session_id, EpisodeOutcome::Unknown.to_u8()])
            .map_err(|e| AgentError::Internal(format!("insert episode: {}", e)))?;
        Ok(id)
    }

    fn label_episode(&self, episode_id: &MemoryNodeId, outcome: EpisodeOutcome, agent_response: &str, error_signature: Option<&str>) -> AgentResult<()> {
        let c = self.lock();
        // Update memory_nodes content
        let mut stmt = c.prepare_cached("SELECT content FROM memory_nodes WHERE id=?1")
            .map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let content: String = stmt.query_row([episode_id], |r| r.get(0))
            .map_err(|e| AgentError::Internal(format!("query: {}", e)))?;
        let mut ep: Episode = serde_json::from_str(&content)
            .map_err(|e| AgentError::Internal(format!("deserialize: {}", e)))?;
        ep.outcome = outcome;
        ep.agent_response = agent_response.to_string();
        ep.error_signature = error_signature.map(|s| s.to_string());
        let new_json = serde_json::to_string(&ep)
            .map_err(|e| AgentError::Internal(format!("serialize: {}", e)))?;
        c.execute("UPDATE memory_nodes SET content=?1, updated_at=datetime('now') WHERE id=?2",
            params![new_json, episode_id])
            .map_err(|e| AgentError::Internal(format!("update: {}", e)))?;
        c.execute("UPDATE episodes SET outcome=?1, error_signature=?2 WHERE id=?3",
            params![outcome.to_u8(), error_signature, episode_id])
            .map_err(|e| AgentError::Internal(format!("update ep: {}", e)))?;
        Ok(())
    }

    fn get_session_episodes(&self, session_id: &str) -> AgentResult<Vec<Episode>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached(
            "SELECT m.content FROM memory_nodes m JOIN episodes e ON m.id=e.id WHERE e.session_id=?1 ORDER BY e.created_at"
        ).map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let rows = stmt.query_map([session_id], Self::get_episode_from_row)
            .map_err(|e| AgentError::Internal(format!("query: {}", e)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn get_episode(&self, id: &MemoryNodeId) -> AgentResult<Option<Episode>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached("SELECT content FROM memory_nodes WHERE id=?1 AND node_type=?2")
            .map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        match stmt.query_row(params![id, MemoryNodeType::Episode.to_u8()], Self::get_episode_from_row) {
            Ok(ep) => Ok(Some(ep)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Internal(format!("query: {}", e))),
        }
    }

    fn record_bug(&self, bug: &Bug) -> AgentResult<MemoryNodeId> {
        let c = self.lock();
        let json = serde_json::to_string(bug).map_err(|e| AgentError::Internal(format!("serialize: {}", e)))?;
        self.insert_node(&c, &bug.id, MemoryNodeType::Bug, &json)?;
        c.execute("INSERT OR REPLACE INTO error_signatures (stack_trace_hash, bug_id, first_seen_at, last_seen_at, occurrence_count) VALUES (?1,?2,datetime('now'),datetime('now'),?3)",
            params![bug.stack_trace_hash, bug.id, bug.occurrence_count])
            .map_err(|e| AgentError::Internal(format!("insert sig: {}", e)))?;
        Ok(bug.id.clone())
    }

    fn record_fix(&self, fix: &Fix, bug_id: &MemoryNodeId) -> AgentResult<MemoryNodeId> {
        let c = self.lock();
        let json = serde_json::to_string(fix).map_err(|e| AgentError::Internal(format!("serialize: {}", e)))?;
        self.insert_node(&c, &fix.id, MemoryNodeType::Fix, &json)?;
        c.execute("INSERT OR IGNORE INTO memory_edges (source_id, target_id, edge_type, confidence) VALUES (?1,?2,?3,1.0)",
            params![bug_id, fix.id, MemoryEdgeType::FixedBy.to_u8()])
            .map_err(|e| AgentError::Internal(format!("insert edge: {}", e)))?;
        Ok(fix.id.clone())
    }

    fn record_fix_no_bug(&self, fix: &Fix) -> AgentResult<MemoryNodeId> {
        let c = self.lock();
        let json = serde_json::to_string(fix).map_err(|e| AgentError::Internal(format!("serialize: {}", e)))?;
        self.insert_node(&c, &fix.id, MemoryNodeType::Fix, &json)?;
        Ok(fix.id.clone())
    }

    fn record_root_cause(&self, rc: &RootCause, bug_ids: &[MemoryNodeId]) -> AgentResult<MemoryNodeId> {
        let c = self.lock();
        let json = serde_json::to_string(rc).map_err(|e| AgentError::Internal(format!("serialize: {}", e)))?;
        self.insert_node(&c, &rc.id, MemoryNodeType::RootCause, &json)?;
        for bug_id in bug_ids {
            c.execute("INSERT OR IGNORE INTO memory_edges (source_id, target_id, edge_type, confidence) VALUES (?1,?2,?3,?4)",
                params![bug_id, rc.id, MemoryEdgeType::CausedBy.to_u8(), rc.confidence])
                .map_err(|e| AgentError::Internal(format!("insert cause edge: {}", e)))?;
        }
        Ok(rc.id.clone())
    }

    fn upsert_insight(&self, insight: &Insight, episode_ids: &[MemoryNodeId]) -> AgentResult<MemoryNodeId> {
        let c = self.lock();
        let json = serde_json::to_string(insight).map_err(|e| AgentError::Internal(format!("serialize: {}", e)))?;
        c.execute("INSERT OR REPLACE INTO memory_nodes (id, node_type, content, content_hash, utility_score, updated_at) VALUES (?1,?2,?3,?4,?5,datetime('now'))",
            params![insight.id, MemoryNodeType::Insight.to_u8(), json, format!("{:x}", sha2::Sha256::new().chain_update(json.as_bytes()).finalize()), insight.utility_score])
            .map_err(|e| AgentError::Internal(format!("upsert insight: {}", e)))?;
        for ep_id in episode_ids {
            c.execute("INSERT OR IGNORE INTO memory_edges (source_id, target_id, edge_type, confidence) VALUES (?1,?2,?3,1.0)",
                params![insight.id, ep_id, MemoryEdgeType::SupportedBy.to_u8()])
                .map_err(|e| AgentError::Internal(format!("insert sup edge: {}", e)))?;
        }
        Ok(insight.id.clone())
    }

    fn get_node(&self, id: &MemoryNodeId) -> AgentResult<Option<MemoryNode>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached("SELECT node_type, content FROM memory_nodes WHERE id=?1")
            .map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        match stmt.query_row([id], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?))) {
            Ok((nt, content)) => {
                let nt = MemoryNodeType::from_u8(nt as u8).unwrap_or(MemoryNodeType::Episode);
                let node = match nt {
                    MemoryNodeType::Episode => MemoryNode::Episode(serde_json::from_str(&content).map_err(|e| AgentError::Internal(format!("Corrupted node {}: {}", id, e)))?),
                    MemoryNodeType::Bug => MemoryNode::Bug(serde_json::from_str(&content).map_err(|e| AgentError::Internal(format!("Corrupted node {}: {}", id, e)))?),
                    MemoryNodeType::Fix => MemoryNode::Fix(serde_json::from_str(&content).map_err(|e| AgentError::Internal(format!("Corrupted node {}: {}", id, e)))?),
                    MemoryNodeType::RootCause => MemoryNode::RootCause(serde_json::from_str(&content).map_err(|e| AgentError::Internal(format!("Corrupted node {}: {}", id, e)))?),
                    MemoryNodeType::Insight => MemoryNode::Insight(serde_json::from_str(&content).map_err(|e| AgentError::Internal(format!("Corrupted node {}: {}", id, e)))?),
                    MemoryNodeType::Preference => MemoryNode::Preference(serde_json::from_str(&content).map_err(|e| AgentError::Internal(format!("Corrupted node {}: {}", id, e)))?),
                    #[allow(unreachable_patterns)]
                    _ => return Ok(None),
                };
                Ok(Some(node))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Internal(format!("query: {}", e))),
        }
    }

    fn get_bug(&self, id: &MemoryNodeId) -> AgentResult<Option<Bug>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached("SELECT content FROM memory_nodes WHERE id=?1 AND node_type=?2")
            .map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        match stmt.query_row(params![id, MemoryNodeType::Bug.to_u8()], Self::get_bug_from_row) {
            Ok(bug) => Ok(Some(bug)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Internal(format!("query: {}", e))),
        }
    }

    fn get_insight(&self, id: &MemoryNodeId) -> AgentResult<Option<Insight>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached("SELECT content FROM memory_nodes WHERE id=?1 AND node_type=?2")
            .map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        match stmt.query_row(params![id, MemoryNodeType::Insight.to_u8()], Self::get_insight_from_row) {
            Ok(ins) => Ok(Some(ins)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Internal(format!("query: {}", e))),
        }
    }

    fn store_embedding(&self, node_id: &MemoryNodeId, vector: &[f32]) -> AgentResult<()> {
        let c = self.lock();
        let bytes: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
        c.execute("INSERT OR REPLACE INTO embedding_store (node_id, vector) VALUES (?1,?2)",
            params![node_id, bytes])
            .map_err(|e| AgentError::Internal(format!("store emb: {}", e)))?;
        Ok(())
    }

    fn get_embedding(&self, node_id: &MemoryNodeId) -> AgentResult<Option<Vec<f32>>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached("SELECT vector FROM embedding_store WHERE node_id=?1")
            .map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        match stmt.query_row([node_id], |row| row.get::<_, Vec<u8>>(0)) {
            Ok(bytes) => {
                let floats: Vec<f32> = bytes.chunks_exact(4).map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap())).collect();
                Ok(Some(floats))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Internal(format!("get emb: {}", e))),
        }
    }

    fn get_all_embeddings(&self, node_type: Option<MemoryNodeType>) -> AgentResult<Vec<(MemoryNodeId, Vec<f32>)>> {
        let c = self.lock();
        let sql = if let Some(_nt) = node_type {
            "SELECT e.node_id, e.vector FROM embedding_store e JOIN memory_nodes n ON e.node_id=n.id WHERE n.node_type=?1"
        } else {
            "SELECT node_id, vector FROM embedding_store"
        };
        let mut stmt = c.prepare_cached(sql).map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let rows: Vec<(String, Vec<u8>)> = if let Some(nt) = node_type {
            stmt.query_map([nt.to_u8()], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| AgentError::Internal(format!("query: {}", e)))?
                .filter_map(|r| r.ok()).collect()
        } else {
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| AgentError::Internal(format!("query: {}", e)))?
                .filter_map(|r| r.ok()).collect()
        };
        Ok(rows.into_iter().map(|(id, bytes)| {
            let floats: Vec<f32> = bytes.chunks_exact(4).map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap())).collect();
            (id, floats)
        }).collect())
    }

    fn get_neighborhood(&self, node_id: &MemoryNodeId, _max_depth: usize) -> AgentResult<Vec<(MemoryNodeId, MemoryEdgeType, f32)>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached(
            "SELECT source_id, target_id, edge_type, confidence FROM memory_edges WHERE source_id=?1 OR target_id=?1"
        ).map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let rows: Vec<(String, String, i32, f32)> = stmt.query_map([node_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(|e| AgentError::Internal(format!("query: {}", e)))?
            .filter_map(|r| r.ok()).collect();

        let mut result = Vec::new();
        for (src, tgt, et, conf) in rows {
            let et = MemoryEdgeType::from_u8(et as u8).unwrap_or(MemoryEdgeType::SimilarTo);
            let neighbor = if src == *node_id { tgt } else { src };
            result.push((neighbor, et, conf));
        }
        Ok(result)
    }

    fn find_similar_bugs(&self, embedding: &[f32], limit: usize) -> AgentResult<Vec<(Bug, f32)>> {
        let all = self.get_all_embeddings(Some(MemoryNodeType::Bug))?;
        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>().clamp(-1.0, 1.0);
        let mut scored: Vec<(MemoryNodeId, f32)> = all.iter()
            .map(|(id, emb)| (id.clone(), dot(embedding, emb)))
            .collect();
        scored.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        let mut results = Vec::new();
        for (id, score) in scored {
            if let Some(bug) = self.get_bug(&id)? {
                results.push((bug, score));
            }
        }
        Ok(results)
    }

    fn get_recent_insights(&self, limit: usize) -> AgentResult<Vec<Insight>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached(
            "SELECT content FROM memory_nodes WHERE node_type=?1 ORDER BY updated_at DESC LIMIT ?2"
        ).map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let rows = stmt.query_map(params![MemoryNodeType::Insight.to_u8(), limit], Self::get_insight_from_row)
            .map_err(|e| AgentError::Internal(format!("query: {}", e)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn find_related_insights(&self, bug_id: &MemoryNodeId) -> AgentResult<Vec<Insight>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached(
            "SELECT m.content FROM memory_nodes m JOIN memory_edges e ON m.id=e.source_id WHERE e.target_id=?1 AND m.node_type=?2"
        ).map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let rows = stmt.query_map(params![bug_id, MemoryNodeType::Insight.to_u8()], Self::get_insight_from_row)
            .map_err(|e| AgentError::Internal(format!("query: {}", e)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn count_similar_patterns(&self, episode: &Episode) -> AgentResult<u32> {
        let c = self.lock();
        if let Some(ref sig) = episode.error_signature {
            let count: i64 = c.query_row("SELECT COUNT(*) FROM error_signatures WHERE stack_trace_hash=?1", [sig], |r| r.get(0))
                .unwrap_or(0);
            Ok(count as u32)
        } else {
            Ok(0)
        }
    }

    fn count_cross_session_occurrences(&self, episode: &Episode) -> AgentResult<u32> {
        let c = self.lock();
        if let Some(ref sig) = episode.error_signature {
            let count: i64 = c.query_row(
                "SELECT COUNT(DISTINCT e.session_id) FROM episodes e JOIN error_signatures es ON e.error_signature=es.stack_trace_hash WHERE es.stack_trace_hash=?1",
                [sig], |r| r.get(0)
            ).unwrap_or(0);
            Ok(count as u32)
        } else {
            Ok(0)
        }
    }

    fn find_bugs_by_signature(&self, error_sig: &str) -> AgentResult<Vec<Bug>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached(
            "SELECT m.content FROM memory_nodes m JOIN error_signatures es ON m.id=es.bug_id WHERE es.stack_trace_hash=?1"
        ).map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let rows = stmt.query_map([error_sig], Self::get_bug_from_row)
            .map_err(|e| AgentError::Internal(format!("query: {}", e)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn get_pending_consolidation_episodes(&self, _min_sessions: u32, min_age_hours: u32) -> AgentResult<Vec<Episode>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached(
            "SELECT m.content FROM memory_nodes m JOIN episodes e ON m.id=e.id WHERE m.node_type=?1 AND e.error_signature IS NOT NULL AND e.created_at < datetime('now', ?2) ORDER BY e.created_at DESC LIMIT 100"
        ).map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let age = format!("-{} hours", min_age_hours);
        let rows = stmt.query_map(params![MemoryNodeType::Episode.to_u8(), age], Self::get_episode_from_row)
            .map_err(|e| AgentError::Internal(format!("query: {}", e)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn get_consolidation_state(&self) -> AgentResult<ConsolidationState> {
        let c = self.lock();
        let state: String = c.query_row("SELECT state FROM consolidation_state WHERE id=1", [], |r| r.get(0))
            .unwrap_or_else(|_| "idle".into());
        Ok(match state.as_str() {
            "running" => ConsolidationState::Running { started_at: chrono::Utc::now() },
            "completed" => ConsolidationState::Completed { insights_generated: 0, pruned_count: 0 },
            "failed" => ConsolidationState::Failed { error: String::new() },
            _ => ConsolidationState::Idle,
        })
    }

    fn set_consolidation_state(&self, state: ConsolidationState) -> AgentResult<()> {
        let c = self.lock();
        match state {
            ConsolidationState::Idle => {
                c.execute("UPDATE consolidation_state SET state='idle', updated_at=datetime('now') WHERE id=1", [])
                    .map_err(|e| AgentError::Internal(format!("set state: {}", e)))?;
            }
            ConsolidationState::Running { started_at } => {
                c.execute("UPDATE consolidation_state SET state='running', started_at=?1, updated_at=datetime('now') WHERE id=1",
                    [started_at.to_rfc3339()])
                    .map_err(|e| AgentError::Internal(format!("set state: {}", e)))?;
            }
            ConsolidationState::Completed { insights_generated, pruned_count } => {
                c.execute("UPDATE consolidation_state SET state='completed', insights_generated=?1, pruned_count=?2, updated_at=datetime('now') WHERE id=1",
                    params![insights_generated, pruned_count])
                    .map_err(|e| AgentError::Internal(format!("set state: {}", e)))?;
            }
            ConsolidationState::Failed { error } => {
                c.execute("UPDATE consolidation_state SET state='failed', error=?1, updated_at=datetime('now') WHERE id=1",
                    [&error])
                    .map_err(|e| AgentError::Internal(format!("set state: {}", e)))?;
            }
        }
        Ok(())
    }

    fn prune_insights(&self, min_utility: f32, max_age_days: u32) -> AgentResult<u32> {
        let c = self.lock();
        let count = c.execute(
            "DELETE FROM memory_nodes WHERE node_type=?1 AND utility_score < ?2 AND updated_at < datetime('now', ?3)",
            params![MemoryNodeType::Insight.to_u8(), min_utility, format!("-{} days", max_age_days)],
        ).map_err(|e| AgentError::Internal(format!("prune: {}", e)))?;
        Ok(count as u32)
    }

    fn prune_episodes(&self, max_age_days: u32) -> AgentResult<u32> {
        let c = self.lock();
        let count = c.execute(
            "DELETE FROM memory_nodes WHERE node_type=?1 AND created_at < datetime('now', ?2) AND id NOT IN (SELECT source_id FROM memory_edges UNION SELECT target_id FROM memory_edges)",
            params![MemoryNodeType::Episode.to_u8(), format!("-{} days", max_age_days)],
        ).map_err(|e| AgentError::Internal(format!("prune episodes: {}", e)))?;
        Ok(count as u32)
    }

    fn find_supporting_episodes(&self, node_id: &MemoryNodeId) -> AgentResult<Vec<MemoryNodeId>> {
        let c = self.lock();
        let mut stmt = c.prepare_cached(
            "SELECT target_id FROM memory_edges WHERE source_id=?1 AND edge_type=?2"
        ).map_err(|e| AgentError::Internal(format!("prep: {}", e)))?;
        let rows: Vec<String> = stmt.query_map(params![node_id, MemoryEdgeType::SupportedBy.to_u8()], |r| r.get(0))
            .map_err(|e| AgentError::Internal(format!("query: {}", e)))?
            .filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    fn record_correction(&self, episode_id: &MemoryNodeId, original: &str, correction: &str) -> AgentResult<()> {
        let c = self.lock();
        let session_id: String = c.query_row("SELECT session_id FROM episodes WHERE id=?1", [episode_id], |r| r.get(0))
            .unwrap_or_default();
        c.execute("INSERT INTO correction_log (session_id, episode_id, original_output, user_feedback) VALUES (?1,?2,?3,?4)",
            params![session_id, episode_id, original, correction])
            .map_err(|e| AgentError::Internal(format!("record correction: {}", e)))?;
        Ok(())
    }

    fn node_count(&self) -> AgentResult<usize> {
        let c = self.lock();
        c.query_row("SELECT COUNT(*) FROM memory_nodes", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize).map_err(|e| AgentError::Internal(format!("count: {}", e)))
    }

    fn edge_count(&self) -> AgentResult<usize> {
        let c = self.lock();
        c.query_row("SELECT COUNT(*) FROM memory_edges", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize).map_err(|e| AgentError::Internal(format!("count: {}", e)))
    }

    fn total_episodes(&self) -> AgentResult<usize> {
        let c = self.lock();
        c.query_row("SELECT COUNT(*) FROM episodes", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize).map_err(|e| AgentError::Internal(format!("count: {}", e)))
    }
}
