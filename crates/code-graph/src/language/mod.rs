use crate::types::{GraphEdge, GraphNode, LanguageId};
use dashmap::DashMap;
use std::sync::Arc;
use tree_sitter::StreamingIterator;

pub mod rust;
pub mod python;
pub mod typescript;
pub mod javascript;
pub mod go;

/// 语言解析能力。每种编程语言实现此 trait。
/// 所有方法接收 tree-sitter Tree + 源文本，返回节点和边。
pub trait Language: Send + Sync {
    /// 语言标识
    fn id(&self) -> LanguageId;

    /// 返回 tree-sitter Language 对象
    fn tree_sitter_language(&self) -> tree_sitter::Language;

    /// 提取函数/方法定义节点
    fn query_functions(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<GraphNode>;

    /// 提取类/结构体/枚举/trait/接口定义节点
    fn query_classes(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<GraphNode>;

    /// 提取导入声明节点
    fn query_imports(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<GraphNode>;

    /// 提取函数调用边
    fn query_calls(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        file_nodes: &[GraphNode],
    ) -> Vec<GraphEdge>;

    /// 提取继承/实现边
    fn query_inheritance(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        file_nodes: &[GraphNode],
    ) -> Vec<GraphEdge>;

    /// 提取引用边 (变量引用、类型引用)
    fn query_references(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        file_nodes: &[GraphNode],
    ) -> Vec<GraphEdge>;

    /// 提取文件级边: Contains, DependsOn, Exports
    fn query_file_edges(
        &self,
        file_node_id: &str,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
    ) -> Vec<GraphEdge>;

    /// 语言特定的文件扩展名列表
    fn extensions(&self) -> &[&str];
}

/// 语言注册表。DashMap 存储，线程安全。
pub struct LanguageRegistry {
    languages: DashMap<LanguageId, Arc<dyn Language>>,
    ext_to_lang: DashMap<String, LanguageId>,
}

impl LanguageRegistry {
    pub fn new() -> Self {
        Self {
            languages: DashMap::new(),
            ext_to_lang: DashMap::new(),
        }
    }

    /// 注册一种语言 (启动时调用)
    pub fn register(&self, lang: Arc<dyn Language>) {
        let id = lang.id();
        for ext in lang.extensions() {
            self.ext_to_lang.insert(ext.to_string(), id);
        }
        self.languages.insert(id, lang);
    }

    /// 从文件扩展名获取语言解析器
    pub fn from_extension(&self, ext: &str) -> Option<Arc<dyn Language>> {
        self.ext_to_lang
            .get(ext)
            .and_then(|id| self.languages.get(&id).map(|l| l.clone()))
    }

    /// 从文件路径检测语言
    pub fn detect(&self, path: &str) -> Option<Arc<dyn Language>> {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())?;
        self.from_extension(ext)
    }

    /// 所有支持的扩展名 (用于 glob 扫描)
    pub fn supported_extensions(&self) -> Vec<String> {
        self.ext_to_lang
            .iter()
            .map(|e| e.key().clone())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.languages.is_empty()
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 辅助: 创建包含所有5种语言的注册表
pub fn create_default_registry() -> LanguageRegistry {
    let registry = LanguageRegistry::new();
    registry.register(Arc::new(rust::RustLanguage));
    registry.register(Arc::new(python::PythonLanguage));
    registry.register(Arc::new(typescript::TypeScriptLanguage));
    registry.register(Arc::new(javascript::JavaScriptLanguage));
    registry.register(Arc::new(go::GoLanguage));
    registry
}

/// 带名称的采集结果（tree-sitter 0.25 中 QueryCapture 无 name 字段，需通过 capture_names 获取）
pub struct NamedCapture<'a> {
    pub node: tree_sitter::Node<'a>,
    pub name: &'a str,
}

/// 辅助: 从 tree-sitter Query 提取节点/边（泛型输出）
pub fn run_query<F, T>(
    query_str: &str,
    language: tree_sitter::Language,
    tree: &tree_sitter::Tree,
    source: &[u8],
    handler: &mut F,
) -> Vec<T>
where
    F: FnMut(&[NamedCapture], &[u8]) -> Option<T>,
{
    let query = match tree_sitter::Query::new(&language, query_str) {
        Ok(q) => q,
        Err(e) => {
            // Use eprintln to ensure visibility in test environments where tracing is not initialized
            eprintln!(
                "tree-sitter query compilation failed: {:?}. Query: {:.200}",
                e, query_str
            );
            return vec![];
        }
    };
    let capture_names: Vec<String> = query.capture_names().iter().map(|s| s.to_string()).collect();
    // Tree-sitter 0.25: use matches() for per-MATCH iteration (not captures() which is per-capture)
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut results = Vec::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source);

    while let Some(m) = matches.next() {
        let named: Vec<NamedCapture> = m
            .captures
            .iter()
            .map(|c| NamedCapture {
                node: c.node,
                name: capture_names
                    .get(c.index as usize)
                    .map(|s| s.as_str())
                    .unwrap_or(""),
            })
            .collect();
        if let Some(item) = handler(&named, source) {
            results.push(item);
        }
    }
    results
}

/// 辅助: 向上查找包含函数节点 (用于确定调用方归属)
pub fn find_enclosing_function(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        let kind = parent.kind();
        if kind == "function_item"
            || kind == "function_definition"
            || kind == "function_declaration"
            || kind == "method_declaration"
            || kind == "arrow_function"
        {
            // 查找函数名
            for i in 0..parent.child_count() {
                if let Some(child) = parent.child(i) {
                    if child.kind() == "identifier"
                        || child.kind() == "field_identifier"
                        || child.kind() == "type_identifier"
                    {
                        return child.utf8_text(source).ok().map(|s| s.to_string());
                    }
                }
            }
            return None; // 找不到名字
        }
        current = parent;
    }
    None
}
