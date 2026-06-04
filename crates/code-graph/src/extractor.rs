use crate::language::Language;
use crate::types::*;
use aegis_core::error::AgentResult;


/// 图提取器。遍历 tree-sitter Tree → 调用 Language trait 各 query_*() → 组装 Vec<Node> + Vec<Edge>。
pub struct GraphExtractor;

impl GraphExtractor {
    /// 从单个文件提取完整图 (节点 + 边)。
    pub fn extract(
        language: &dyn Language,
        tree: &tree_sitter::Tree,
        source: &str,
        file_path: &str,
        file_hash: &str,
    ) -> AgentResult<(Vec<GraphNode>, Vec<GraphEdge>)> {
        let source_bytes = source.as_bytes();
        let normalized_path = file_path.replace('\\', "/");

        // Step 1: 创建文件节点（根节点，唯一标识整个文件）
        let file_node_id = make_node_id(&normalized_path, &normalized_path, "File", 1);
        let total_lines = source.lines().count() as u32;
        let file_node = GraphNode {
            id: file_node_id.clone(),
            node_type: NodeType::File,
            file_path: normalized_path.clone(),
            name: normalized_path.clone(),
            start_line: 1,
            start_col: 1,
            end_line: total_lines.max(1),
            end_col: 1,
            visibility: Visibility::Public,
            metadata: serde_json::json!({"hash": file_hash, "size_bytes": source.len()}),
        };

        // Step 2: 按序提取各类节点 (Language trait 方法)
        let functions = language.query_functions(tree, source_bytes, &normalized_path);
        let classes = language.query_classes(tree, source_bytes, &normalized_path);
        let imports = language.query_imports(tree, source_bytes, &normalized_path);

        let mut nodes = vec![file_node];
        nodes.extend(functions);
        nodes.extend(classes);
        nodes.extend(imports);

        // Step 3: 提取各类边（调用依赖节点列表）
        let calls = language.query_calls(tree, source_bytes, &normalized_path, &nodes);
        let inheritance = language.query_inheritance(tree, source_bytes, &normalized_path, &nodes);
        let references = language.query_references(tree, source_bytes, &normalized_path, &nodes);

        // Step 4: 文件级边 (Contains, DependsOn, Exports)
        let mut edges = Vec::with_capacity(calls.len() + inheritance.len() + references.len());
        edges.extend(calls);
        edges.extend(inheritance);
        edges.extend(references);

        // 在 file_node_id 存在时构建文件级边
        if let Some(file_node) = nodes.first() {
            let file_edges =
                language.query_file_edges(&file_node.id, &nodes, &edges);
            edges.extend(file_edges);
        }

        Ok((nodes, edges))
    }
}
