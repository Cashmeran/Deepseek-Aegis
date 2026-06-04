use crate::language::run_query;
use crate::types::*;

pub struct GoLanguage;

impl crate::language::Language for GoLanguage {
    fn id(&self) -> LanguageId { LanguageId::Go }
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
    }
    fn extensions(&self) -> &[&str] { &["go"] }

    fn query_functions(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(function_declaration name: (identifier) @fn.name parameters: (parameter_list)? @fn.params) @fn.def
(method_declaration receiver: (parameter_list) @method.recv name: (field_identifier) @method.name parameters: (parameter_list)? @method.params) @method.def
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_go::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            let mut params = String::new();
            let mut recv = String::new();
            let (mut sl, mut sc, mut el, mut ec) = (0, 0, 0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);
                let (erow, ecol) = (cap.node.end_position().row as u32 + 1, cap.node.end_position().column as u32 + 1);

                match cap.name {
                    "fn.name" | "method.name" => { name = text.to_string(); sl = row; sc = col; }
                    "fn.params" | "method.params" => params = text.to_string(),
                    "method.recv" => recv = text.to_string(),
                    "fn.def" | "method.def" => { el = erow; ec = ecol; }
                    _ => {}
                }
            }

            if !name.is_empty() {
                let is_public = name.chars().next().map_or(false, |c| c.is_uppercase());
                Some(GraphNode {
                    id: make_node_id(file_path, &name, "Function", sl),
                    node_type: NodeType::Function, file_path: file_path.to_string(),
                    visibility: if is_public { Visibility::Public } else { Visibility::Private },
                    name,
                    start_line: sl, start_col: sc, end_line: el, end_col: ec,
                    metadata: serde_json::json!({"params": params, "receiver": recv}),
                })
            } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_classes(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(type_declaration name: (type_identifier) @struct.name type: (struct_type field_declaration_list: (field_declaration_list)? @struct.fields)) @struct.def
(type_declaration name: (type_identifier) @iface.name type: (interface_type) @iface.body) @iface.def
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_go::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            let mut has_fields = false;
            let (mut sl, mut sc, mut el, mut ec) = (0, 0, 0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);
                let (erow, ecol) = (cap.node.end_position().row as u32 + 1, cap.node.end_position().column as u32 + 1);

                match cap.name {
                    "struct.name" | "iface.name" => { name = text.to_string(); sl = row; sc = col; }
                    "struct.fields" => has_fields = true,
                    "struct.def" | "iface.def" => { el = erow; ec = ecol; }
                    _ => {}
                }
            }

            if !name.is_empty() {
                let is_public = name.chars().next().map_or(false, |c| c.is_uppercase());
                Some(GraphNode {
                    id: make_node_id(file_path, &name, "Struct", sl),
                    node_type: NodeType::Struct, file_path: file_path.to_string(),
                    visibility: if is_public { Visibility::Public } else { Visibility::Private },
                    name,
                    start_line: sl, start_col: sc, end_line: el, end_col: ec,
                    metadata: serde_json::json!({"has_fields": has_fields}),
                })
            } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_imports(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let query_str = r#"
(import_declaration (import_spec name: (package_identifier)? @imp.alias path: (interpreted_string_literal) @imp.path)) @imp.single
(import_declaration (import_spec_list (import_spec name: (package_identifier)? @imp.alias path: (interpreted_string_literal) @imp.path)+)) @imp.group
"#;
        let mut nodes = Vec::new();
        run_query(query_str, tree_sitter_go::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut path = String::new();
            let mut alias = String::new();
            let (mut sl, mut sc) = (0, 0);

            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);

                match cap.name {
                    "imp.path" => { path = text.trim_matches('"').to_string(); sl = row; sc = col; }
                    "imp.alias" => alias = text.to_string(),
                    _ => {}
                }
            }

            let display = if alias.is_empty() {
                path.rsplit('/').next().unwrap_or(&path).to_string()
            } else { alias.clone() };

            if !display.is_empty() {
                let display_clone = display.clone();
                Some(GraphNode {
                    id: make_node_id(file_path, &display_clone, "Import", sl),
                    node_type: NodeType::Import, file_path: file_path.to_string(), name: display,
                    start_line: sl, start_col: sc, end_line: sl, end_col: sc + 10,
                    visibility: Visibility::Private,
                    metadata: serde_json::json!({"package_path": path, "alias": alias}),
                })
            } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_calls(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let query_str = r#"
(call_expression function: (identifier) @call.callee) @call.expr
(call_expression function: (selector_expression field: (field_identifier) @call.method)) @call.meth
"#;
        let mut edges = Vec::new();
        run_query(query_str, tree_sitter_go::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut callee = String::new();
            for cap in captures {
                match cap.name {
                    "call.callee" | "call.method" => callee = cap.node.utf8_text(source).unwrap_or("").to_string(),
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

    fn query_inheritance(&self, _tree: &tree_sitter::Tree, _source: &[u8], _file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        // Go 没有显式继承。Interface 实现是隐式的。
        // 后续可选集成 gopls LSP 获取 interface 实现关系。
        vec![]
    }

    fn query_references(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let query_str = r#"
(var_declaration (var_spec name: (identifier) @var.name)) @var.def
(short_var_declaration left: (identifier) @short.name) @short.def
"#;
        let mut edges = Vec::new();
        run_query(query_str, tree_sitter_go::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new();
            for cap in captures {
                if cap.name == "var.name" || cap.name == "short.name" {
                    name = cap.node.utf8_text(source).unwrap_or("").to_string();
                }
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
