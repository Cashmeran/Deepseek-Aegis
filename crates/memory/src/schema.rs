//! SQLite DDL for the causal memory system.
//! 7 tables + 9 indexes. WAL mode. Normalized design.

pub const MEMORY_DDL: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -8000;

CREATE TABLE IF NOT EXISTS memory_nodes (
    id TEXT PRIMARY KEY NOT NULL,
    node_type INTEGER NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    utility_score REAL NOT NULL DEFAULT 0.0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS memory_edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL REFERENCES memory_nodes(id) ON DELETE CASCADE,
    target_id TEXT NOT NULL REFERENCES memory_nodes(id) ON DELETE CASCADE,
    edge_type INTEGER NOT NULL,
    confidence REAL NOT NULL DEFAULT 1.0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(source_id, target_id, edge_type)
);

CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY REFERENCES memory_nodes(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    outcome INTEGER NOT NULL,
    error_signature TEXT,
    files_modified TEXT,
    token_usage INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS error_signatures (
    stack_trace_hash TEXT PRIMARY KEY,
    bug_id TEXT NOT NULL REFERENCES memory_nodes(id),
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    occurrence_count INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS correction_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    episode_id TEXT REFERENCES memory_nodes(id),
    original_output TEXT NOT NULL,
    user_feedback TEXT NOT NULL,
    extracted_insight TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS embedding_store (
    node_id TEXT PRIMARY KEY REFERENCES memory_nodes(id) ON DELETE CASCADE,
    dimension INTEGER NOT NULL DEFAULT 384,
    vector BLOB NOT NULL,
    indexed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS consolidation_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    state TEXT NOT NULL DEFAULT 'idle',
    started_at TEXT,
    insights_generated INTEGER NOT NULL DEFAULT 0,
    pruned_count INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT OR IGNORE INTO consolidation_state (id, state) VALUES (1, 'idle');

CREATE INDEX IF NOT EXISTS idx_memory_nodes_type ON memory_nodes(node_type);
CREATE INDEX IF NOT EXISTS idx_memory_nodes_hash ON memory_nodes(content_hash);
CREATE INDEX IF NOT EXISTS idx_memory_edges_source ON memory_edges(source_id);
CREATE INDEX IF NOT EXISTS idx_memory_edges_target ON memory_edges(target_id);
CREATE INDEX IF NOT EXISTS idx_memory_edges_type ON memory_edges(edge_type);
CREATE INDEX IF NOT EXISTS idx_episodes_session ON episodes(session_id);
CREATE INDEX IF NOT EXISTS idx_episodes_outcome ON episodes(outcome);
CREATE INDEX IF NOT EXISTS idx_episodes_error ON episodes(error_signature);
CREATE INDEX IF NOT EXISTS idx_correction_session ON correction_log(session_id);
";
