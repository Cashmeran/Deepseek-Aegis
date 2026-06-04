use crate::language::run_query;
use crate::types::*;

pub struct JavaScriptLanguage;

impl crate::language::Language for JavaScriptLanguage {
    fn id(&self) -> LanguageId { LanguageId::JavaScript }
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_javascript::LANGUAGE.into()
    }
    fn extensions(&self) -> &[&str] { &["js", "jsx"] }

    fn query_functions(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(function_declaration name: (identifier) @fn.name parameters: (formal_parameters)? @fn.params) @fn.def
(variable_declarator name: (identifier) @fn.arrow_name value: (arrow_function parameters: (formal_parameters)? @fn.arrow_params) @fn.arrow_body) @fn.arrow_def
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_javascript::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            let mut params = String::new();
            let mut is_async = false;
            let (mut sl, mut sc, mut el, mut ec) = (0, 0, 0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);
                let (erow, ecol) = (cap.node.end_position().row as u32 + 1, cap.node.end_position().column as u32 + 1);

                match cap.name {
                    "fn.name" | "fn.arrow_name" => { name = text.to_string(); sl = row; sc = col; }
                    "fn.params" | "fn.arrow_params" => params = text.to_string(),
                    "fn.async" => is_async = true,
                    "fn.def" | "fn.arrow_def" | "fn.async_def" => { el = erow; ec = ecol; }
                    _ => {}
                }
            }

            if !name.is_empty() {
                Some(GraphNode {
                    id: make_node_id(file_path, &name, "Function", sl),
                    node_type: NodeType::Function, file_path: file_path.to_string(), name,
                    start_line: sl, start_col: sc, end_line: el, end_col: ec,
                    visibility: Visibility::Public,
                    metadata: serde_json::json!({"params": params, "async": is_async}),
                })
            } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_classes(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(class_declaration name: (identifier) @class.name extends: (extends_clause (identifier) @class.parent)?) @class.def
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_javascript::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            let mut parent = String::new();
            let (mut sl, mut sc, mut el, mut ec) = (0, 0, 0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);
                let (erow, ecol) = (cap.node.end_position().row as u32 + 1, cap.node.end_position().column as u32 + 1);

                match cap.name {
                    "class.name" => { name = text.to_string(); sl = row; sc = col; }
                    "class.parent" => parent = text.to_string(),
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
                    metadata: serde_json::json!({"extends": parent}),
                })
            } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_imports(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(import_statement source: (string) @imp.source import_clause: (import_clause (identifier)? @imp.default)?) @imp.stmt
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_javascript::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut from = String::new();
            let mut name = String::new();
            let (mut sl, mut sc) = (0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);

                match cap.name {
                    "imp.source" => { from = text.trim_matches('"').trim_matches('\'').to_string(); }
                    "imp.default" => { name = text.to_string(); sl = row; sc = col; }
                    _ => {}
                }
            }

            let display = if name.is_empty() { from.clone() } else { format!("{}.{}", from, name) };
            if !display.is_empty() {
                Some(GraphNode {
                    id: make_node_id(file_path, &display, "Import", sl),
                    node_type: NodeType::Import, file_path: file_path.to_string(), name: display,
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
(call_expression function: (identifier) @call.callee) @call.expr
(call_expression function: (member_expression property: (property_identifier) @call.method)) @call.meth
(new_expression constructor: (identifier) @new.class) @new.expr
"#;
        let mut edges = Vec::new();
        run_query(query_str, tree_sitter_javascript::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut callee = String::new();
            for cap in captures {
                match cap.name {
                    "call.callee" | "call.method" | "new.class" => callee = cap.node.utf8_text(source).unwrap_or("").to_string(),
                    _ => {}
                }
            }
            if !callee.is_empty() {
                let caller = make_node_id(file_path, "__enclosing__", "Function", 1);
                Some(GraphEdge { source_id: caller, target_id: make_node_id(file_path, &callee, "Function", 1), edge_type: EdgeType::Calls, weight: 0.8, target_name: String::new() })
            } else { None }
        }).iter().for_each(|e| edges.push(e.clone()));
        edges
    }

    fn query_inheritance(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let mut edges = Vec::new();
        let query_str = r#"
(class_declaration name: (identifier) @class.name extends: (extends_clause (identifier) @class.parent)+) @class.ext
"#;
        run_query(query_str, tree_sitter_javascript::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut child = String::new();
            let mut parents = Vec::new();
            for cap in captures {
                match cap.name {
                    "class.name" => child = cap.node.utf8_text(source).unwrap_or("").to_string(),
                    "class.parent" => parents.push(cap.node.utf8_text(source).unwrap_or("").to_string()),
                    _ => {}
                }
            }
            if !child.is_empty() {
                for p in &parents {
                    let src = make_node_id(file_path, &child, "Class", 1);
                    let tgt = make_node_id(file_path, p, "Class", 1);
                    edges.push(GraphEdge { source_id: src, target_id: tgt, edge_type: EdgeType::Inherits, weight: 0.9, target_name: String::new() });
                }
            }
            Some(GraphNode { id: String::new(), node_type: NodeType::Function, file_path: String::new(), name: String::new(), start_line: 0, start_col: 0, end_line: 0, end_col: 0, visibility: Visibility::Private, metadata: serde_json::json!({}) })
        });
        edges
    }

    fn query_references(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let query_str = r#"
(variable_declarator name: (identifier) @var.name) @var.def
"#;
        let mut edges = Vec::new();
        run_query(query_str, tree_sitter_javascript::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            for cap in captures {
                if cap.name == "var.name" { name = cap.node.utf8_text(source).unwrap_or("").to_string(); }
            }
            if !name.is_empty() {
                let src = make_node_id(file_path, "__enclosing__", "Function", 1);
                Some(GraphEdge { source_id: src, target_id: make_node_id(file_path, &name, "Variable", 1), edge_type: EdgeType::References, weight: 0.5, target_name: String::new() })
            } else { None }
        }).iter().for_each(|e| edges.push(e.clone()));
        edges
    }

    fn query_file_edges(&self, file_node_id: &str, nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<GraphEdge> {
        let mut file_edges = Vec::new();
        let file_path = nodes.first().map(|n| n.file_path.clone()).unwrap_or_default();
        for node in nodes {
            if node.node_type != NodeType::File {
                file_edges.push(GraphEdge { source_id: file_node_id.to_string(), target_id: node.id.clone(), edge_type: EdgeType::Contains, weight: 1.0, target_name: String::new() });
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
            file_edges.push(GraphEdge { source_id: file_node_id.to_string(), target_id: make_node_id(&dep_file, &dep_file, "File", 1), edge_type: EdgeType::DependsOn, weight: 0.7, target_name: String::new() });
        }
        file_edges
    }
}
