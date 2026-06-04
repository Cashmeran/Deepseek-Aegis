use crate::language::LanguageRegistry;
use aegis_core::error::{AgentError, AgentResult};
use std::sync::Arc;
use tree_sitter::{Parser, Tree};

/// 代码解析器。管理 tree-sitter 解析实例。
pub struct CodeParser {
    registry: Arc<LanguageRegistry>,
}

impl CodeParser {
    pub fn new(registry: Arc<LanguageRegistry>) -> Self {
        Self { registry }
    }

    /// 从文件路径检测语言
    pub fn detect_language(&self, path: &str) -> Option<Arc<dyn crate::language::Language>> {
        self.registry.detect(path)
    }

    /// 解析单个文件为 tree-sitter Tree。
    /// `old_tree` = 上次解析结果，用于增量解析加速（仅重新解析变更区域）。
    pub fn parse(
        &self,
        source: &str,
        language: &dyn crate::language::Language,
        old_tree: Option<&Tree>,
    ) -> AgentResult<Tree> {
        let mut parser = Parser::new();
        parser
            .set_language(&language.tree_sitter_language())
            .map_err(|e| {
                AgentError::ConfigError(format!("tree-sitter language registration failed: {}", e))
            })?;

        // 增量解析: 提供旧树 → tree-sitter 仅重新词法分析 + 语法分析变更的字符区域
        let tree = parser.parse(source, old_tree).ok_or_else(|| {
            AgentError::Internal("tree-sitter parse returned None".into())
        })?;

        Ok(tree)
    }

    /// 统计语法错误节点数（ERROR 或 MISSING 节点）。
    /// `> 10` → 可能语言检测错误或文件严重损坏。
    pub fn count_parse_errors(tree: &Tree, source: &[u8]) -> usize {
        Self::count_errors_recursive(&tree.root_node(), source)
    }

    fn count_errors_recursive(node: &tree_sitter::Node, source: &[u8]) -> usize {
        let mut count = 0;
        let kind = node.kind();
        if kind == "ERROR" || node.is_missing() {
            count += 1;
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                count += Self::count_errors_recursive(&child, source);
            }
        }
        count
    }
}
