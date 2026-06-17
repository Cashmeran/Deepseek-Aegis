use crate::language::{find_enclosing_function, run_query, NamedCapture};
use crate::types::*;

pub struct RustLanguage;

/// 辅助: 从单实体捕获中提取 GraphNode
fn extract_node(captures: &[NamedCapture], source: &[u8], file_path: &str, node_type: NodeType) -> Option<GraphNode> {
    let mut name = String::new();
    let (mut sl, mut sc, mut el, mut ec) = (0, 0, 0, 0);
    let mut visibility = Visibility::Private;
    for cap in captures {
        let text = cap.node.utf8_text(source).unwrap_or("");
        let (row, col) = (cap.node.start_position().row as u32 + 1, cap.node.start_position().column as u32 + 1);
        let (erow, ecol) = (cap.node.end_position().row as u32 + 1, cap.node.end_position().column as u32 + 1);
        match cap.name {
            "name" | "fn.name" | "method.name" => { name = text.to_string(); sl = row; sc = col; }
            "def" | "fn.def" | "method.def" => { el = erow; ec = ecol; }
            "fn.vis" | "method.vis" => visibility = Visibility::Public,
            "fn.params" | "method.params" => {}
            _ => {}
        }
    }
    if !name.is_empty() {
        Some(GraphNode {
            id: make_node_id(file_path, &name, &format!("{:?}", node_type), sl),
            node_type, file_path: file_path.to_string(), name,
            start_line: sl, start_col: sc, end_line: el, end_col: ec,
            visibility, metadata: serde_json::json!({}),
        })
    } else { None }
}

impl crate::language::Language for RustLanguage {
    fn id(&self) -> LanguageId { LanguageId::Rust }
    fn tree_sitter_language(&self) -> tree_sitter::Language { tree_sitter_rust::LANGUAGE.into() }
    fn extensions(&self) -> &[&str] { &["rs"] }

    fn query_functions(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let mut nodes = Vec::new();
        // Top-level functions (with optional visibility modifier for pub detection)
        run_query("(function_item (visibility_modifier)? @fn.vis name: (identifier) @fn.name parameters: (parameters) @fn.params) @fn.def",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name = String::new(); let mut params = String::new(); let mut vis = Visibility::Private;
            let (mut sl, mut sc, mut el, mut ec) = (0,0,0,0);
            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                let (r,c) = (cap.node.start_position().row as u32+1, cap.node.start_position().column as u32+1);
                let (er,ec2) = (cap.node.end_position().row as u32+1, cap.node.end_position().column as u32+1);
                match cap.name {
                    "fn.name" => { name=text.to_string(); sl=r; sc=c; }
                    "fn.params" => { params=text.to_string(); }
                    "fn.vis" => { vis=Visibility::Public; }
                    "fn.def" => { el=er; ec=ec2; }
                    _ => {}
                }
            }
            if !name.is_empty() { Some(GraphNode{ id:make_node_id(file_path,&name,"Function",sl), node_type:NodeType::Function, file_path:file_path.to_string(), name, start_line:sl,start_col:sc,end_line:el,end_col:ec, visibility:vis, metadata:serde_json::json!({"params":params})}) } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        // Methods in impl blocks
        run_query("(impl_item type: (type_identifier) @impl.type body: (declaration_list (function_item (visibility_modifier)? @method.vis name: (identifier) @method.name parameters: (parameters) @method.params) @method.def))",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name=String::new(); let mut params=String::new(); let mut vis=Visibility::Private; let mut imp=String::new();
            let (mut sl,mut sc,mut el,mut ec)=(0,0,0,0);
            for cap in captures {
                let text=cap.node.utf8_text(source).unwrap_or("");
                let (r,c)=(cap.node.start_position().row as u32+1,cap.node.start_position().column as u32+1);
                let (er,ec2)=(cap.node.end_position().row as u32+1,cap.node.end_position().column as u32+1);
                match cap.name {
                    "method.name"=>{name=text.to_string();sl=r;sc=c;}
                    "method.params"=>{params=text.to_string();}
                    "method.vis"=>{vis=Visibility::Public;}
                    "impl.type"=>{imp=text.to_string();}
                    "method.def"=>{el=er;ec=ec2;}
                    _=>{}
                }
            }
            if !name.is_empty() { Some(GraphNode{ id:make_node_id(file_path,&name,"Method",sl), node_type:NodeType::Function, file_path:file_path.to_string(), name, start_line:sl,start_col:sc,end_line:el,end_col:ec, visibility:vis, metadata:serde_json::json!({"params":params,"impl_for":imp})}) } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_classes(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let mut nodes = Vec::new();
        let struct_q = "(struct_item name: (type_identifier) @name) @def";
        run_query(struct_q, tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |c,s| extract_node(c,s,file_path,NodeType::Struct)).iter().for_each(|n| nodes.push(n.clone()));
        let enum_q = "(enum_item name: (type_identifier) @name) @def";
        run_query(enum_q, tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |c,s| extract_node(c,s,file_path,NodeType::Struct)).iter().for_each(|n| nodes.push(n.clone()));
        let trait_q = "(trait_item name: (type_identifier) @name) @def";
        run_query(trait_q, tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |c,s| extract_node(c,s,file_path,NodeType::Struct)).iter().for_each(|n| nodes.push(n.clone()));
        let type_q = "(type_item name: (type_identifier) @name) @def";
        run_query(type_q, tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |c,s| extract_node(c,s,file_path,NodeType::TypeAlias)).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_imports(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str) -> Vec<GraphNode> {
        let mut nodes = Vec::new();
        run_query("(use_declaration argument: (scoped_identifier) @path) @stmt",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut path = String::new(); let mut sl=0; let mut sc=0;
            for cap in captures {
                let text = cap.node.utf8_text(source).unwrap_or("");
                if cap.name == "path" { path = text.to_string(); sl = cap.node.start_position().row as u32+1; sc = cap.node.start_position().column as u32+1; }
            }
            if !path.is_empty() { Some(GraphNode{ id:make_node_id(file_path,&path,"Import",sl), node_type:NodeType::Import, file_path:file_path.to_string(), name:path, start_line:sl,start_col:sc,end_line:sl,end_col:sc+10, visibility:Visibility::Private, metadata:serde_json::json!({}) }) } else { None }
        }).iter().for_each(|n| nodes.push(n.clone()));
        run_query("(mod_item name: (identifier) @name) @def",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |c,s| extract_node(c,s,file_path,NodeType::Import)).iter().for_each(|n| nodes.push(n.clone()));
        nodes
    }

    fn query_calls(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let mut edges = Vec::new();
        run_query("(call_expression function: (identifier) @callee)",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut callee = String::new(); let mut call_node = None;
            for cap in captures {
                if cap.name == "callee" { callee = cap.node.utf8_text(source).unwrap_or("").to_string(); call_node = Some(cap.node); }
            }
            if !callee.is_empty() && let Some(cn) = call_node && let Some(enclosing) = find_enclosing_function(cn, source) {
                return Some(GraphEdge{ source_id:make_node_id(file_path,&enclosing,"Function",1), target_id:make_node_id(file_path,&callee,"Function",1), edge_type:EdgeType::Calls, weight:0.8, target_name: callee.clone() });
            } None
        }).iter().for_each(|e| edges.push(e.clone()));
        run_query("(call_expression function: (field_expression field: (field_identifier) @method))",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut callee=String::new(); let mut call_node=None;
            for cap in captures { if cap.name=="method" { callee=cap.node.utf8_text(source).unwrap_or("").to_string(); call_node=Some(cap.node); } }
            if !callee.is_empty() && let Some(cn)=call_node && let Some(e)=find_enclosing_function(cn,source) {
                return Some(GraphEdge{ source_id:make_node_id(file_path,&e,"Function",1), target_id:make_node_id(file_path,&callee,"Function",1), edge_type:EdgeType::Calls, weight:0.7, target_name: callee.clone() });
            } None
        }).iter().for_each(|e| edges.push(e.clone()));
        run_query("(macro_invocation macro: (identifier) @macro)",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut callee=String::new(); let mut cn=None;
            for cap in captures { if cap.name=="macro" { callee=cap.node.utf8_text(source).unwrap_or("").to_string(); cn=Some(cap.node); } }
            if !callee.is_empty() && let Some(c)=cn && let Some(e)=find_enclosing_function(c,source) {
                return Some(GraphEdge{ source_id:make_node_id(file_path,&e,"Function",1), target_id:make_node_id(file_path,&callee,"Function",1), edge_type:EdgeType::Calls, weight:0.3, target_name: callee.clone() });
            } None
        }).iter().for_each(|e| edges.push(e.clone()));
        edges
    }

    fn query_inheritance(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let mut edges = Vec::new();
        run_query("(impl_item trait: (type_identifier) @trait type: (type_identifier) @impl)",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut trait_name=String::new(); let mut impl_name=String::new();
            for cap in captures {
                let text=cap.node.utf8_text(source).unwrap_or("");
                match cap.name { "trait"=>trait_name=text.to_string(), "impl"=>impl_name=text.to_string(), _=>{} }
            }
            if !trait_name.is_empty() && !impl_name.is_empty() {
                Some(GraphEdge{ source_id:make_node_id(file_path,&impl_name,"Struct",1), target_id:make_node_id(file_path,&trait_name,"Struct",1), edge_type:EdgeType::Inherits, weight:0.9, target_name: String::new() })
            } else { None }
        }).iter().for_each(|e| edges.push(e.clone()));
        edges
    }

    fn query_references(&self, tree: &tree_sitter::Tree, source: &[u8], file_path: &str, _file_nodes: &[GraphNode]) -> Vec<GraphEdge> {
        let mut edges = Vec::new();
        run_query("(let_declaration pattern: (identifier) @var)",
            tree_sitter_rust::LANGUAGE.into(), tree, source, &mut |captures, source| {
            let mut name=String::new(); let mut node=None;
            for cap in captures { if cap.name=="var" { name=cap.node.utf8_text(source).unwrap_or("").to_string(); node=Some(cap.node); } }
            if !name.is_empty() && let Some(n) = node && let Some(e) = find_enclosing_function(n, source) {
                return Some(GraphEdge{ source_id:make_node_id(file_path,&e,"Function",1), target_id:make_node_id(file_path,&name,"Variable",1), edge_type:EdgeType::References, weight:0.5, target_name: String::new() });
            } None
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
            file_edges.push(GraphEdge { source_id: file_node_id.to_string(), target_id: make_node_id(&dep_file,&dep_file,"File",1), edge_type: EdgeType::DependsOn, weight: 0.7, target_name: String::new() });
        }
        file_edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{Language, run_query};
    use tree_sitter::{Parser, StreamingIterator};

    fn parse_rust(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn test_simple_function() {
        let source = "fn main() {}";
        let tree = parse_rust(source);
        // Test each query separately
        let fn_nodes: Vec<GraphNode> = run_query(
            "(function_item name: (identifier) @fn.name parameters: (parameters) @fn.params) @fn.def",
            tree_sitter_rust::LANGUAGE.into(), &tree, source.as_bytes(),
            &mut |captures, src| {
                for cap in captures {
                    if cap.name == "fn.name" {
                        let text = cap.node.utf8_text(src).unwrap_or("");
                        let sl = cap.node.start_position().row as u32 + 1;
                        return Some(GraphNode {
                            id: make_node_id("test.rs", text, "Function", sl),
                            node_type: NodeType::Function, file_path: "test.rs".into(),
                            name: text.to_string(), start_line: sl, start_col: 1,
                            end_line: 1, end_col: 1, visibility: Visibility::Private,
                            metadata: serde_json::json!({}),
                        });
                    }
                }
                None
            },
        );
        assert_eq!(fn_nodes.len(), 1, "Expected 1 function from raw run_query");

        let lang = RustLanguage;
        let nodes = lang.query_functions(&tree, source.as_bytes(), "test.rs");
        assert_eq!(nodes.len(), 1, "Expected 1 function from query_functions, got {}", nodes.len());
        assert_eq!(nodes[0].name, "main");
    }

    #[test]
    fn test_pub_function() {
        let source = "pub fn init() {}";
        let tree = parse_rust(source);
        let lang = RustLanguage;
        let nodes = lang.query_functions(&tree, source.as_bytes(), "test.rs");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].visibility, Visibility::Public);
    }

    #[test]
    fn test_method() {
        let source = "impl Foo { fn new() -> Self { Foo {} } }";
        let tree = parse_rust(source);
        let lang = RustLanguage;
        let nodes = lang.query_functions(&tree, source.as_bytes(), "test.rs");
        assert!(!nodes.is_empty(), "Should find method new");
    }

    #[test]
    fn test_struct() {
        let tree = parse_rust("pub struct User { name: String }");
        let nodes = RustLanguage.query_classes(&tree, "pub struct User { name: String }".as_bytes(), "test.rs");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "User");
    }

    #[test]
    fn test_enum() {
        let tree = parse_rust("pub enum Color { Red, Green }");
        let nodes = RustLanguage.query_classes(&tree, "pub enum Color { Red, Green }".as_bytes(), "test.rs");
        assert!(!nodes.is_empty());
    }

    #[test]
    fn test_trait() {
        let tree = parse_rust("pub trait Display { fn fmt(&self) -> String; }");
        let nodes = RustLanguage.query_classes(&tree, "pub trait Display { fn fmt(&self) -> String; }".as_bytes(), "test.rs");
        assert!(!nodes.is_empty());
    }

    #[test]
    fn test_use_imports() {
        let tree = parse_rust("use std::collections::HashMap;\nuse crate::utils;");
        let nodes = RustLanguage.query_imports(&tree, "use std::collections::HashMap;\nuse crate::utils;".as_bytes(), "test.rs");
        assert!(nodes.len() >= 2);
    }

    #[test]
    fn test_empty() {
        let tree = parse_rust("");
        assert!(RustLanguage.query_functions(&tree, "".as_bytes(), "test.rs").is_empty());
    }

    #[test]
    fn test_syntax_error() {
        let tree = parse_rust("fn broken( {");
        let nodes = RustLanguage.query_functions(&tree, "fn broken( {".as_bytes(), "test.rs");
        assert!(nodes.is_empty());
    }

    #[test]
    fn test_multiple_functions() {
        let tree = parse_rust("fn a() {}\nfn b() {}\npub fn c() {}");
        let nodes = RustLanguage.query_functions(&tree, "fn a() {}\nfn b() {}\npub fn c() {}".as_bytes(), "test.rs");
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn test_function_calls() {
        let source = "fn main() { helper(); } fn helper() {}";
        let tree = parse_rust(source);
        let fn_nodes = RustLanguage.query_functions(&tree, source.as_bytes(), "test.rs");
        let calls = RustLanguage.query_calls(&tree, source.as_bytes(), "test.rs", &fn_nodes);
        assert!(!calls.is_empty());
    }
}
