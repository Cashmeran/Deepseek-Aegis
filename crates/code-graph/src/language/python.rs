use crate::language::run_query;
use crate::types::*;

pub struct PythonLanguage;

impl crate::language::Language for PythonLanguage {
    fn id(&self) -> LanguageId { LanguageId::Python }
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }
    fn extensions(&self) -> &[&str] { &["py"] }

    fn query_functions(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(function_definition name: (identifier) @fn.name parameters: (parameters)? @fn.params) @fn.def
(function_definition name: (identifier) @fn.name parameters: (parameters)? @fn.params) @fn.def
(decorated_definition
  definition: (function_definition name: (identifier) @fn.name parameters: (parameters)? @fn.params) @fn.decorated_def) @fn.decorated
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_python::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            let mut params = String::new();
            let mut decorators = Vec::new();
            let mut is_async = false;
            let (mut sl, mut sc, mut el, mut ec) = (0, 0, 0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);
                let (erow, ecol) = (cap.node.end_position().row as u32 + 1, cap.node.end_position().column as u32 + 1);

                match cap.name {
                    "fn.name" => { name = text.to_string(); sl = row; sc = col; }
                    "fn.params" => params = text.to_string(),
                    "fn.async" => is_async = true,
                    "decorator.name" => decorators.push(text.to_string()),
                    "fn.def" | "fn.async_def" | "fn.decorated_def" => { el = erow; ec = ecol; }
                    _ => {}
                }
            }

            if !name.is_empty() {
                Some(GraphNode {
                    id: make_node_id(file_path, &name, "Function", sl),
                    node_type: NodeType::Function, file_path: file_path.to_string(), name,
                    start_line: sl, start_col: sc, end_line: el, end_col: ec,
                    visibility: Visibility::Public, // Python默认公开
                    metadata: serde_json::json!({"params": params, "async": is_async, "decorators": decorators}),
                })
            } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_classes(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(class_definition name: (identifier) @class.name superclasses: (argument_list (identifier) @class.parent)*) @class.def
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_python::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            let mut parents = Vec::new();
            let (mut sl, mut sc, mut el, mut ec) = (0, 0, 0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);
                let (erow, ecol) = (cap.node.end_position().row as u32 + 1, cap.node.end_position().column as u32 + 1);

                match cap.name {
                    "class.name" => { name = text.to_string(); sl = row; sc = col; }
                    "class.parent" => parents.push(text.to_string()),
                    "class.def" => { el = erow; ec = ecol; }
                    _ => {}
                }
            }

            if !name.is_empty() {
                Some(GraphNode {
                    id: make_node_id(file_path, &name, "Class", sl),
                    node_type: NodeType::Struct, file_path: file_path.to_string(), name,
                    start_line: sl, start_col: sc, end_line: el, end_col: ec,
                    visibility: Visibility::Public,
                    metadata: serde_json::json!({"parent_classes": parents}),
                })
            } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_imports(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(import_statement name: (dotted_name) @imp.module) @imp.stmt
(import_from_statement module_name: (dotted_name) @imp.from name: (dotted_name) @imp.name) @imp.from_stmt
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_python::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            let mut from = String::new();
            let (mut sl, mut sc) = (0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);
                match cap.name {
                    "imp.module" => { name = text.to_string(); sl = row; sc = col; }
                    "imp.from" => from = text.to_string(),
                    "imp.name" => { name = format!("{}.{}", from, text); sl = row; sc = col; }
                    _ => {}
                }
            }

            if !name.is_empty() {
                Some(GraphNode {
                    id: make_node_id(file_path, &name, "Import", sl),
                    node_type: NodeType::Import, file_path: file_path.to_string(), name,
                    start_line: sl, start_col: sc, end_line: sl, end_col: sc + 10,
                    visibility: Visibility::Private,
                    metadata: serde_json::json!({"from": from}),
                })
            } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_calls(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let query_str = r#"
(call function: (identifier) @call.callee) @call.expr
(call function: (attribute object: (_) @call.recv attribute: (identifier) @call.method)) @call.meth
"#;
        let mut edges = Vec::new();
        run_query(query_str, tree_sitter_python::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut callee = String::new();
            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                match cap.name {
                    "call.callee" | "call.method" => callee = text.to_string(),
                    _ => {}
                }
            }
            if !callee.is_empty() {
                let caller_id = make_node_id(file_path, "__enclosing__", "Function", 1);
                let target_id = make_node_id(file_path, &callee, "Function", 1);
                Some(GraphEdge { source_id: caller_id, target_id, edge_type: EdgeType::Calls, weight: 0.8, target_name: String::new() })
            } else { None }
        }).iter().for_each(|e| edges.push(e.clone()));
        edges
    }

    fn query_inheritance(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let mut edges = Vec::new();
        // Python inheritance is extracted from class_definition superclasses
        let query_str = r#"
(class_definition name: (identifier) @class.name superclasses: (argument_list (identifier) @class.parent)+) @class.def
"#;
        run_query(query_str, tree_sitter_python::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut child = String::new();
            let mut parents = Vec::new();
            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                match cap.name {
                    "class.name" => child = text.to_string(),
                    "class.parent" => parents.push(text.to_string()),
                    _ => {}
                }
            }
            if !child.is_empty() {
                for parent in &parents {
                    let source_id = make_node_id(file_path, &child, "Class", 1);
                    let target_id = make_node_id(file_path, parent, "Class", 1);
                    edges.push(GraphEdge { source_id, target_id, edge_type: EdgeType::Inherits, weight: 0.9, target_name: String::new() });
                }
            }
            Some(GraphNode {
                id: String::new(), node_type: NodeType::Function, file_path: String::new(),
                name: String::new(), start_line: 0, start_col: 0, end_line: 0, end_col: 0,
                visibility: Visibility::Private, metadata: serde_json::json!({}),
            })
        });
        edges
    }

    fn query_references(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let query_str = r#"
(assignment left: (identifier) @assign.target) @assign.stmt
"#;
        let mut edges = Vec::new();
        run_query(query_str, tree_sitter_python::LANGUAGE.into(), tree, source, &mut |captures, _source| {
            let mut target = String::new();
            for cap in captures {
                if cap.name == "assign.target" {
                    target = cap.node.utf8_text(source).unwrap_or("").to_string();
                }
            }
            if !target.is_empty() {
                let source_id = make_node_id(file_path, "__enclosing__", "Function", 1);
                let target_id = make_node_id(file_path, &target, "Variable", 1);
                Some(GraphEdge { source_id, target_id, edge_type: EdgeType::References, weight: 0.5, target_name: String::new() })
            } else { None }
        }).iter().for_each(|e| edges.push(e.clone()));
        edges
    }

    fn query_file_edges(&self, file_node_id: &str, nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<GraphEdge> {
        let mut file_edges = Vec::new();
        let file_path = nodes.first().map(|n| n.file_path.clone()).unwrap_or_default();

        for node in nodes {
            if node.node_type != NodeType::File {
                file_edges.push(GraphEdge {
                    source_id: file_node_id.to_string(), target_id: node.id.clone(),
                    edge_type: EdgeType::Contains, weight: 1.0, target_name: String::new() });
            }
        }

        let mut dep_files = std::collections::HashSet::new();
        for edge in edges {
            for node in nodes {
                if node.id == edge.target_id && node.file_path != file_path {
                    dep_files.insert(node.file_path.clone());
                }
            }
        }
        for dep_file in dep_files {
            file_edges.push(GraphEdge {
                source_id: file_node_id.to_string(),
                target_id: make_node_id(&dep_file, &dep_file, "File", 1),
                edge_type: EdgeType::DependsOn, weight: 0.7, target_name: String::new() });
        }
        file_edges
    }
}
