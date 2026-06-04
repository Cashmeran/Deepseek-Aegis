use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

// ═══════════════════════════════════════════════════════════
// 节点类型 — 6种，覆盖5语言的所有关键语法结构
// 参考 SCIP SymbolInformation.kind 分类
// ═══════════════════════════════════════════════════════════

/// 节点类型。编码为 u8 存入 SQLite，节省存储 + 加速索引。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeType {
    File = 0,
    Function = 1,
    Struct = 2,
    Import = 3,
    Variable = 4,
    TypeAlias = 5,
}

impl NodeType {
    pub fn from_u8(n: u8) -> Option<Self> {
        match n {
            0 => Some(Self::File),
            1 => Some(Self::Function),
            2 => Some(Self::Struct),
            3 => Some(Self::Import),
            4 => Some(Self::Variable),
            5 => Some(Self::TypeAlias),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

// ═══════════════════════════════════════════════════════════
// 边类型 — 8种，覆盖代码关系全谱
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EdgeType {
    Contains = 0,
    Imports = 1,
    Calls = 2,
    Inherits = 3,
    References = 4,
    Exports = 5,
    DependsOn = 6,
    DefinesType = 7,
}

impl EdgeType {
    pub fn from_u8(n: u8) -> Option<Self> {
        match n {
            0 => Some(Self::Contains),
            1 => Some(Self::Imports),
            2 => Some(Self::Calls),
            3 => Some(Self::Inherits),
            4 => Some(Self::References),
            5 => Some(Self::Exports),
            6 => Some(Self::DependsOn),
            7 => Some(Self::DefinesType),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

// ═══════════════════════════════════════════════════════════
// 核心数据结构
// ═══════════════════════════════════════════════════════════

/// 节点ID: 64字符 SHA-256 hex
pub type NodeId = String;

/// 图节点。所有语言实体归一化为统一结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// 节点唯一ID: SHA-256("{file_path}::{name}::{node_type}::{line}")
    pub id: NodeId,
    pub node_type: NodeType,
    /// 所属文件 (相对workspace路径，正斜杠)
    pub file_path: String,
    /// 符号名 (函数名/类名/模块名)
    pub name: String,
    /// 1-based 起始行
    pub start_line: u32,
    /// 1-based 起始列
    pub start_col: u32,
    /// 1-based 结束行
    pub end_line: u32,
    /// 1-based 结束列
    pub end_col: u32,
    pub visibility: Visibility,
    /// 语言特定元数据 (参数列表/返回类型/装饰器/docs)
    pub metadata: serde_json::Value,
}

/// 图边。有向，带权重。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source_id: NodeId,
    pub target_id: NodeId,
    pub edge_type: EdgeType,
    /// 0.0-1.0 关系强度 (CALLS=0.8核心调用, IMPORTS=0.5模块级)
    pub weight: f32,
    /// 目标符号名 (用于跨文件解析)
    #[serde(default)]
    pub target_name: String,
}

impl GraphEdge {
    pub fn new(source_id: NodeId, target_id: NodeId, edge_type: EdgeType, weight: f32) -> Self {
        Self { source_id, target_id, edge_type, weight, target_name: String::new() }
    }
    pub fn with_target_name(mut self, name: String) -> Self {
        self.target_name = name;
        self
    }
}

/// 邻域查询结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborhoodResult {
    pub center: GraphNode,
    /// 谁依赖我 (edge: source→center)
    pub incoming: Vec<(GraphEdge, GraphNode)>,
    /// 我依赖谁 (edge: center→target)
    pub outgoing: Vec<(GraphEdge, GraphNode)>,
}

/// BFS遍历结果
#[derive(Debug, Clone)]
pub struct BfsNode {
    pub node: GraphNode,
    pub depth: usize,
    /// 从哪个节点到达的 (用于路径回溯)
    pub incoming_from: Option<String>,
}

// ═══════════════════════════════════════════════════════════
// 辅助类型
// ═══════════════════════════════════════════════════════════

/// 可见性
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Visibility {
    Public = 0,
    Crate = 1,
    Private = 2,
}

/// 语言标识
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LanguageId {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
}

impl LanguageId {
    /// 从文件扩展名检测语言
    pub fn from_extension(path: &str) -> Option<Self> {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())?;
        match ext {
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            "ts" | "tsx" => Some(Self::TypeScript),
            "js" | "jsx" => Some(Self::JavaScript),
            "go" => Some(Self::Go),
            _ => None,
        }
    }
}

/// 文件索引元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    pub path: PathBuf,
    pub language: LanguageId,
    /// SHA-256 hex (64 chars)
    pub hash: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub indexed_at: chrono::DateTime<chrono::Utc>,
    pub parse_error_count: usize,
}

/// 文件变更结果
#[derive(Debug)]
pub enum FileChange {
    Unchanged,
    Updated {
        nodes_added: usize,
        edges_added: usize,
        parse_errors: usize,
    },
}

/// 全量扫描结果
#[derive(Debug, Clone)]
pub struct FullScanResult {
    pub total_files: usize,
    pub updated: usize,
    pub skipped: usize,
    pub removed: usize,
    pub errors: usize,
    pub first_error: Option<String>,
    pub elapsed_ms: u64,
}

// ═══════════════════════════════════════════════════════════
// NodeId 生成
// ═══════════════════════════════════════════════════════════

/// 生成确定性的 NodeId。
/// 公式: SHA-256("{file_path}::{name}::{node_type}::{line}")
pub fn make_node_id(file_path: &str, name: &str, node_type: &str, line: u32) -> NodeId {
    let input = format!("{}::{}::{}::{}", file_path, name, node_type, line);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ═══════════════════════════════════════════════════════════
// tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_roundtrip() {
        for nt in [
            NodeType::File,
            NodeType::Function,
            NodeType::Struct,
            NodeType::Import,
            NodeType::Variable,
            NodeType::TypeAlias,
        ] {
            assert_eq!(NodeType::from_u8(nt.to_u8()), Some(nt));
        }
        assert_eq!(NodeType::from_u8(255), None);
    }

    #[test]
    fn test_edge_type_roundtrip() {
        for et in [
            EdgeType::Contains,
            EdgeType::Imports,
            EdgeType::Calls,
            EdgeType::Inherits,
            EdgeType::References,
            EdgeType::Exports,
            EdgeType::DependsOn,
            EdgeType::DefinesType,
        ] {
            assert_eq!(EdgeType::from_u8(et.to_u8()), Some(et));
        }
        assert_eq!(EdgeType::from_u8(255), None);
    }

    #[test]
    fn test_node_id_deterministic() {
        let id1 = make_node_id("src/main.rs", "main", "Function", 10);
        let id2 = make_node_id("src/main.rs", "main", "Function", 10);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64);
        assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_node_id_different_inputs() {
        let id1 = make_node_id("src/a.rs", "foo", "Function", 1);
        let id2 = make_node_id("src/b.rs", "foo", "Function", 1);
        assert_ne!(id1, id2);

        let id3 = make_node_id("src/a.rs", "bar", "Function", 1);
        assert_ne!(id1, id3);

        let id4 = make_node_id("src/a.rs", "foo", "Struct", 1);
        assert_ne!(id1, id4);
    }

    #[test]
    fn test_language_id_from_extension() {
        assert_eq!(LanguageId::from_extension("foo.rs"), Some(LanguageId::Rust));
        assert_eq!(
            LanguageId::from_extension("foo.py"),
            Some(LanguageId::Python)
        );
        assert_eq!(
            LanguageId::from_extension("foo.ts"),
            Some(LanguageId::TypeScript)
        );
        assert_eq!(
            LanguageId::from_extension("foo.tsx"),
            Some(LanguageId::TypeScript)
        );
        assert_eq!(
            LanguageId::from_extension("foo.js"),
            Some(LanguageId::JavaScript)
        );
        assert_eq!(
            LanguageId::from_extension("foo.jsx"),
            Some(LanguageId::JavaScript)
        );
        assert_eq!(LanguageId::from_extension("foo.go"), Some(LanguageId::Go));
        assert_eq!(LanguageId::from_extension("foo.txt"), None);
        assert_eq!(LanguageId::from_extension("Makefile"), None);
    }

    #[test]
    fn test_visibility_serde() {
        let v = Visibility::Public;
        let json = serde_json::to_string(&v).unwrap();
        let back: Visibility = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Visibility::Public);
    }

    #[test]
    fn test_graph_node_serde_roundtrip() {
        let node = GraphNode {
            id: make_node_id("test.rs", "main", "Function", 1),
            node_type: NodeType::Function,
            file_path: "test.rs".into(),
            name: "main".into(),
            start_line: 1,
            start_col: 1,
            end_line: 5,
            end_col: 2,
            visibility: Visibility::Public,
            metadata: serde_json::json!({"params": "()", "ret": "()"}),
        };
        let json = serde_json::to_string(&node).unwrap();
        let back: GraphNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, node.id);
        assert_eq!(back.name, "main");
        assert_eq!(back.node_type, NodeType::Function);
    }
}
