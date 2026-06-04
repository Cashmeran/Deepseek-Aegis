-- Code Graph: 代码知识图谱的 SQLite Schema
-- 节点: 文件(File), 类(Class), 函数(Function), 变量(Variable)
-- 边: IMPORTS, CALLS, INHERITS, CONTAINS, REFERENCES

CREATE TABLE IF NOT EXISTS nodes (
    id         TEXT PRIMARY KEY,      -- UUID v4
    node_type  TEXT NOT NULL,         -- file|class|function|variable
    name       TEXT NOT NULL,         -- 符号名称 (如 "main", "User")
    file_path  TEXT NOT NULL,         -- 所属文件路径
    line_start INTEGER,               -- 定义起始行
    line_end   INTEGER,               -- 定义结束行
    metadata   TEXT DEFAULT '{}',     -- JSON 扩展字段
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS edges (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id  TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    target_id  TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    edge_type  TEXT NOT NULL,         -- imports|calls|inherits|contains|references
    metadata   TEXT DEFAULT '{}',     -- JSON 扩展字段
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_nodes_file_path ON nodes(file_path);
CREATE INDEX IF NOT EXISTS idx_nodes_type ON nodes(node_type);
CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);
