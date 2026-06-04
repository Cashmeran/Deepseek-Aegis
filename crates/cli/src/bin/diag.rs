//! aegis-diag — CLI 诊断工具，无需 TUI，直接验证。
//!
//! 用法:
//!   cargo run --bin aegis-diag -- check-graph [--db <PATH>]
//!   cargo run --bin aegis-diag -- dump-edges [--db <PATH>]
//!   cargo run --bin aegis-diag -- test <PROMPT>

use std::path::PathBuf;
use std::sync::Arc;

use aegis_code_graph::GraphStore;
use aegis_memory::MemoryStore;

// ── check-graph ────────────────────────────────────────────────────

fn cmd_check_graph(db: Option<PathBuf>) -> anyhow::Result<()> {
    let db_path = db.unwrap_or_else(|| PathBuf::from(".agent/code_graph.db"));

    if !db_path.exists() {
        println!("DB not found at {}, running full scan...", db_path.display());
        let cwd = std::env::current_dir().unwrap_or_default();
        let store = aegis_code_graph::SqliteGraphStore::open(&db_path)?;
        let store: Arc<dyn GraphStore> = Arc::new(store);
        let lang_registry = Arc::new(aegis_code_graph::create_default_registry());
        let parser = Arc::new(aegis_code_graph::CodeParser::new(lang_registry.clone()));
        let indexer = aegis_code_graph::IncrementalIndexer::new(
            Arc::clone(&store), parser, lang_registry,
        );
        let result = indexer.full_scan(&cwd)?;
        println!("Scan: {} files, {} updated, {} skipped, {} errors, {}ms",
            result.total_files, result.updated, result.skipped, result.errors, result.elapsed_ms);
        if let Some(e) = &result.first_error { println!("First error: {e}"); }
    }

    let store = aegis_code_graph::SqliteGraphStore::open(&db_path)?;
    let n_nodes = store.node_count().unwrap_or(0);
    let n_edges = store.edge_count().unwrap_or(0);
    let files = store.list_files().unwrap_or_default();
    println!("Nodes: {n_nodes}  Edges: {n_edges}  Files: {}\n", files.len());

    for file_path in &files {
        let path_str = file_path.to_string_lossy().replace('\\', "/");
        match aegis_code_graph::get_architectural_context(&store, &path_str) {
            Ok(ctx) => {
                let lines: Vec<_> = ctx.lines().collect();
                println!("── {} ({} lines) ──", path_str, lines.len());
                for line in lines.iter().take(10) { println!("  {line}"); }
                if lines.len() > 10 { println!("  ... {} more lines", lines.len() - 10); }
                println!();
            }
            Err(e) => println!("x {} — {e}\n", path_str),
        }
    }

    // Impact map check — use direct SQL for accurate counts
    println!("── Impact map ──");
    let db_path2 = db_path.clone();
    let conn = rusqlite::Connection::open(&db_path2)?;

    // Count edges where target is a Function or Struct (project-internal calls)
    let internal_calls: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edges e
         JOIN nodes n ON e.target_id = n.id
         WHERE e.edge_type = 2 AND n.node_type IN (1, 2)",
        [], |r| r.get(0)
    )?;

    // Count edges with project-external targets (stdlib etc)
    let external_calls: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edges WHERE edge_type = 2 AND target_id NOT IN (SELECT id FROM nodes)",
        [], |r| r.get(0)
    )?;

    println!("  Internal (resolved) call edges: {internal_calls}");
    println!("  External (stdlib) call edges:   {external_calls}");

    // Test impact_map on a few project functions that should have callers
    let fns: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT n.name, n.file_path FROM nodes n
             JOIN edges e ON n.id = e.target_id
             WHERE e.edge_type = 2 AND n.node_type = 1
             LIMIT 10"
        )?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok()).collect()
    };

    for (name, _file) in &fns {
        match aegis_code_graph::get_impact_map(&store, name) {
            Ok(r) => {
                let summary = r.lines().next().unwrap_or("");
                println!("  impact_map({name}): {summary}");
            }
            Err(e) => println!("  impact_map({name}) — error: {e}"),
        }
    }
    drop(conn);

    Ok(())
}

// ── dump-edges ─────────────────────────────────────────────────────

fn cmd_dump_edges(db: Option<PathBuf>) -> anyhow::Result<()> {
    let db_path = db.unwrap_or_else(|| PathBuf::from(".agent/code_graph.db"));
    if !db_path.exists() { anyhow::bail!("DB not found at {}", db_path.display()); }

    let conn = rusqlite::Connection::open(&db_path)?;
    // Query edges with resolved target info via JOIN
    let mut stmt = conn.prepare(
        "SELECT n1.file_path || '::' || n1.name,
                e.target_name,
                COALESCE(n2.file_path || '::' || n2.name, 'x UNRESOLVED'),
                e.weight
         FROM edges e
         JOIN nodes n1 ON e.source_id = n1.id
         LEFT JOIN nodes n2 ON e.target_id = n2.id
         WHERE e.edge_type = 2
         ORDER BY n1.file_path, n1.name
         LIMIT 200"
    )?;
    let rows = stmt.query_map([], |row| Ok((
        row.get::<_, String>(0)?, row.get::<_, String>(1)?,
        row.get::<_, String>(2)?, row.get::<_, f32>(3)?,
    )))?;

    let (mut count, mut _unresolved) = (0usize, 0usize);
    for row in rows {
        let (src, tgt_name, tgt, weight) = row?;
        let is_unresolved = tgt.contains("UNRESOLVED");
        if is_unresolved { _unresolved += 1; }
        // Only show unresolved ones or first few resolved
        if is_unresolved || count < 20 {
            println!("{src} → \"{tgt_name}\" → {tgt}  (w={weight})");
        }
        count += 1;
    }

    // Get full stats
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM edges WHERE edge_type=2", [], |r| r.get(0))?;
    let resolved_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edges e JOIN nodes n ON e.target_id = n.id WHERE e.edge_type=2", [], |r| r.get(0)
    )?;

    let unres = total - resolved_count;
    println!("\nTotal call edges: {total}, resolved: {resolved_count}, unresolved: {unres} (shown: {count})");
    Ok(())
}

// ── test ───────────────────────────────────────────────────────────

fn cmd_test(prompt: &str) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;

        let mut config = aegis_core::types::config::AgentConfig::default();
        config.verify_before_output = false;
        config.max_turns = 15;

        let model = config.default_model.clone();
        let llm = Arc::new(aegis_core::llm::deepseek::DeepSeekClient::new(api_key, &model)?);
        let registry = Arc::new(aegis_core::tool_system::registry::ToolRegistry::new());
        let sp = Arc::new(aegis_core::agent::system_prompt::SystemPromptBuilder::new(config.clone()));

        // Standard tools
        use aegis_tools::*;
        registry.register(Arc::new(diagnostics::DiagnosticsTool))?;
        registry.register(Arc::new(list_dir::ListDirTool))?;
        registry.register(Arc::new(file_read::FileReadTool::new()))?;
        registry.register(Arc::new(glob::GlobTool::new()))?;
        registry.register(Arc::new(grep::GrepTool::new()))?;
        registry.register(Arc::new(bash::BashTool::new()))?;
        registry.register(Arc::new(file_write::FileWriteTool::new()))?;
        registry.register(Arc::new(file_edit::FileEditTool::new()))?;
        registry.register(Arc::new(run_tests::RunTestsTool))?;

        // Code graph tools
        let db_path = PathBuf::from(".agent/code_graph.db");
        if db_path.exists() {
            registry.register(Arc::new(LazyDiagTool { db_path: db_path.clone() }))?;
            registry.register(Arc::new(ImpactDiagTool { db_path: db_path.clone() }))?;
        }

        let mut agent = aegis_core::agent::AgentLoop::new(config, llm, registry, sp);

        // Sandbox
        use aegis_core::types::sandbox::SandboxBackend;
        let backend = aegis_sandbox::ProcessBackend;
        if let Ok(instance) = backend.spawn(aegis_core::types::sandbox::SandboxPermissions::read_only_workspace(".")) {
            agent = agent.with_sandbox(Arc::new(std::sync::Mutex::new(instance)));
        }

        // Memory
        let memory_db_path = PathBuf::from(".agent/memory.db");
        if let Ok(_store) = <aegis_memory::SqliteMemoryStore as aegis_memory::MemoryStore>::open(&memory_db_path) {
            agent = agent.with_memory(Arc::new({
                let mdb = memory_db_path.clone();
                move |query: &str| -> String {
                    if !mdb.exists() { return String::new(); }
                    match <aegis_memory::SqliteMemoryStore as aegis_memory::MemoryStore>::open(&mdb) {
                        Ok(store) => {
                            let mut results = Vec::new();
                            if let Ok(bugs) = store.find_bugs_by_signature(query) {
                                for b in bugs.iter().take(3) {
                                    results.push(format!("[bug] {}: {}", b.error_message, b.description));
                                }
                            }
                            results.join("\n")
                        }
                        Err(_) => String::new(),
                    }
                }
            }));
        }

        // Graph context
        let graph_db = PathBuf::from(".agent/code_graph.db");
        agent = agent.with_graph(Arc::new(move |query: &str| -> String {
            if !graph_db.exists() { return String::new(); }
            aegis_code_graph::SqliteGraphStore::open(&graph_db)
                .ok()
                .and_then(|store| aegis_code_graph::get_architectural_context(&store, query).ok())
                .unwrap_or_default()
        }));

        println!(">>> {prompt}\n");

        let result = agent.run_streaming(prompt, &|event: aegis_core::llm::client::StreamEvent| {
            match event {
                aegis_core::llm::client::StreamEvent::TextDelta(text) => {
                    print!("{text}");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
                aegis_core::llm::client::StreamEvent::ThinkingDelta(_) => {}
                aegis_core::llm::client::StreamEvent::ToolUseStart { name, input, .. } => {
                    println!("\n  … {} {}", name, serde_json::to_string(&input).unwrap_or_default());
                }
                aegis_core::llm::client::StreamEvent::ToolResult { name, is_error, output, elapsed_ms, .. } => {
                    let icon = if is_error { "x" } else { "+" };
                    let preview: String = output.lines().take(2).collect::<Vec<_>>().join(" | ");
                    println!("  {icon} {name} | {elapsed_ms}ms | {preview}");
                }
                aegis_core::llm::client::StreamEvent::Done(resp) => {
                    println!("\n── Done: {} in / {} out", resp.usage.input_tokens, resp.usage.output_tokens);
                }
                _ => {}
            }
        }).await;

        match result {
            Ok(o) => println!("\n[OK] confidence={:?} len={}", o.confidence, o.content.len()),
            Err(e) => eprintln!("\n[ERROR] {e}"),
        }
        Ok(())
    })
}

// ── Tool wrappers ───────────────────────────────────────────────────

use aegis_core::types::{ContentBlock, ConcurrencySafety, RiskLevel, Tool, ToolContext, ToolMetadata, ToolResultMessage, ToolSchema, ToolUse};
use async_trait::async_trait;

struct LazyDiagTool { db_path: PathBuf }

#[async_trait]
impl Tool for LazyDiagTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> aegis_core::error::AgentResult<ToolResultMessage> {
        let file_path = tool_use.input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let start = std::time::Instant::now();
        let store = aegis_code_graph::SqliteGraphStore::open(&self.db_path)?;
        let cwd = std::env::current_dir().unwrap_or_default();
        let abs = cwd.join(file_path);
        let text = aegis_code_graph::get_architectural_context(&store, &abs.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|e| format!("Error: {e}"));
        let truncated: String = text.lines().take(40).collect::<Vec<_>>().join("\n");
        Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: truncated }],
            elapsed_ms: start.elapsed().as_millis() as u64 })
    }
}
impl ToolMetadata for LazyDiagTool {
    fn schema(&self) -> ToolSchema { ToolSchema { name: "get_architectural_context".into(), description: "Get file architectural context".into(), prompt: String::new(), input_schema: serde_json::json!({"type":"object","properties":{"file_path":{"type":"string"}},"required":["file_path"]}) } }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

struct ImpactDiagTool { db_path: PathBuf }

#[async_trait]
impl Tool for ImpactDiagTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> aegis_core::error::AgentResult<ToolResultMessage> {
        let symbol = tool_use.input.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
        let start = std::time::Instant::now();
        let store = aegis_code_graph::SqliteGraphStore::open(&self.db_path)?;
        let text = aegis_code_graph::get_impact_map(&store, symbol).unwrap_or_else(|e| format!("Error: {e}"));
        Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text }], elapsed_ms: start.elapsed().as_millis() as u64 })
    }
}
impl ToolMetadata for ImpactDiagTool {
    fn schema(&self) -> ToolSchema { ToolSchema { name: "impact_map".into(), description: "Show blast radius for a symbol".into(), prompt: String::new(), input_schema: serde_json::json!({"type":"object","properties":{"symbol":{"type":"string"}},"required":["symbol"]}) } }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

fn cmd_debug_sql(db: Option<PathBuf>) -> anyhow::Result<()> {
    let db_path = db.unwrap_or_else(|| PathBuf::from(".agent/code_graph.db"));
    if !db_path.exists() { anyhow::bail!("DB not found at {}", db_path.display()); }

    let conn = rusqlite::Connection::open(&db_path)?;

    // Check schema
    println!("── Columns in edges table ──");
    let mut stmt = conn.prepare("PRAGMA table_info(edges)")?;
    let cols: Vec<(String, String)> = stmt.query_map([], |row| Ok((row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok()).collect();
    for (name, typ) in &cols { println!("  {name}: {typ}"); }

    // Count edges with target_name
    println!("\n── Edge stats ──");
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
    let with_name: i64 = conn.query_row("SELECT COUNT(*) FROM edges WHERE target_name != ''", [], |r| r.get(0))?;
    let dangling: i64 = conn.query_row("SELECT COUNT(*) FROM edges WHERE edge_type=2 AND target_id NOT IN (SELECT id FROM nodes)", [], |r| r.get(0))?;
    let call_with_name: i64 = conn.query_row("SELECT COUNT(*) FROM edges WHERE edge_type=2 AND target_name != ''", [], |r| r.get(0))?;
    println!("  Total edges: {total}");
    println!("  Edges with target_name: {with_name}");
    println!("  Call edges with target_name: {call_with_name}");
    println!("  Dangling call edges: {dangling}");

    // Sample resolution
    println!("\n── Sample resolution test ──");
    let mut stmt = conn.prepare(
        "SELECT e.target_name, n.name as def_name, n.file_path as def_file
         FROM edges e, nodes n
         WHERE e.edge_type = 2
           AND e.target_name != ''
           AND n.name = e.target_name
           AND n.node_type IN (1, 2)
         LIMIT 10"
    )?;
    let matches: Vec<(String, String, String)> = stmt.query_map([], |row| Ok((
        row.get(0)?, row.get(1)?, row.get(2)?
    )))?.filter_map(|r| r.ok()).collect();

    if matches.is_empty() {
        println!("  NO MATCHES: target_name doesn't match any node names!");
        // Show some sample target_names and node names
        let mut s = conn.prepare("SELECT target_name FROM edges WHERE edge_type=2 AND target_name != '' LIMIT 10")?;
        println!("  Sample target_names:");
        for row in s.query_map([], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()) {
            println!("    '{}'", row);
        }
        let mut s = conn.prepare("SELECT name, node_type FROM nodes WHERE node_type IN (1,2) LIMIT 10")?;
        println!("  Sample function/struct nodes:");
        for row in s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i32>(1)?)))?.filter_map(|r| r.ok()) {
            println!("    '{}' (type={})", row.0, row.1);
        }
    } else {
        for (tgt_name, def_name, def_file) in &matches {
            println!("  '{tgt_name}' → {def_name} @ {def_file}");
        }
    }

    Ok(())
}

// ── main ───────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("check-graph") => cmd_check_graph(args.get(2).map(PathBuf::from)),
        Some("dump-edges") => cmd_dump_edges(args.get(2).map(PathBuf::from)),
        Some("debug-sql") => cmd_debug_sql(args.get(2).map(PathBuf::from)),
        Some("test") => {
            let prompt = args.get(2).map(|s| s.as_str()).unwrap_or("Hello");
            cmd_test(prompt)
        }
        _ => {
            eprintln!("Usage: aegis-diag <check-graph|dump-edges|test> [args]");
            Ok(())
        }
    }
}
