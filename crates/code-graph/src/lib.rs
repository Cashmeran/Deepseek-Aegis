// aegis-code-graph: Persistent code knowledge graph
// tree-sitter parsing → 5-language AST extraction → SQLite WAL storage → incremental updates
//
// Public API:
//   GraphStore trait          — graph persistence abstraction
//   SqliteGraphStore          — SQLite implementation
//   CodeParser                — tree-sitter parser
//   GraphExtractor            — AST→graph extractor
//   IncrementalIndexer        — background scan + incremental updates
//   ArchitecturalContextTool  — MCP tool (impl Tool trait)

pub mod types;
pub mod language;
pub mod parser;
pub mod extractor;
pub mod store;
pub mod query;
pub mod incremental;
pub mod mcp_tool;

// Core trait re-exports
pub use store::GraphStore;
pub use store::{SqliteGraphStore, VizEdge, VizNode};

// High-level API re-exports
pub use parser::CodeParser;
pub use extractor::GraphExtractor;
pub use types::{FileChange, FullScanResult};
pub use incremental::IncrementalIndexer;
pub use query::{detect_project_root, get_architectural_context, get_codebase_overview, get_impact_map};
pub use language::{create_default_registry, Language, LanguageRegistry};
pub use types::*;
