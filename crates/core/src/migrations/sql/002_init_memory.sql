-- Memory Store: 因果记忆系统的 SQLite Schema
-- 节点: Episode, Bug, Fix, RootCause, Insight
-- 边: caused_by, fixed_by, derived_from, related_to

CREATE TABLE IF NOT EXISTS memory_nodes (
    id          TEXT PRIMARY KEY,      -- UUID v4
    node_type   TEXT NOT NULL,         -- episode|bug|fix|root_cause|insight
    title       TEXT NOT NULL,         -- 人类可读标题
    content     TEXT NOT NULL,         -- 完整文本内容
    embedding   BLOB,                  -- 384d 浮点向量 (fastembed ONNX)
    session_id  TEXT,                  -- 来源会话
    severity    TEXT DEFAULT 'medium', -- low|medium|high|critical
    resolved    INTEGER DEFAULT 0,     -- 0=未解决, 1=已解决
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS memory_edges (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id   TEXT NOT NULL REFERENCES memory_nodes(id) ON DELETE CASCADE,
    target_id   TEXT NOT NULL REFERENCES memory_nodes(id) ON DELETE CASCADE,
    edge_type   TEXT NOT NULL,         -- caused_by|fixed_by|derived_from|related_to
    weight      REAL DEFAULT 1.0,      -- 边权重 (用于 PPR 传播)
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_memory_nodes_type ON memory_nodes(node_type);
CREATE INDEX IF NOT EXISTS idx_memory_nodes_session ON memory_nodes(session_id);
CREATE INDEX IF NOT EXISTS idx_memory_nodes_resolved ON memory_nodes(resolved);
CREATE INDEX IF NOT EXISTS idx_memory_edges_source ON memory_edges(source_id);
CREATE INDEX IF NOT EXISTS idx_memory_edges_type ON memory_edges(edge_type);
