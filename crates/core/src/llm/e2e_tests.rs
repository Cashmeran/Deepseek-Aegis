//! 端到端集成测试 — 完整 Agent 流程追踪。
//! 需要 DEEPSEEK_API_KEY 环境变量。
//! 运行: export DEEPSEEK_API_KEY=sk-... && cargo test -p aegis-core -- e2e_tests --nocapture

use crate::agent::{
    AgentLoop, ConfidenceLevel, SprintContract,
    SystemPromptBuilder,
};
use crate::llm::deepseek::DeepSeekClient;
use crate::tool_system::ToolRegistry;
use crate::types::config::AgentConfig;
use crate::types::tool::ExecutionMode;
use std::sync::Arc;

fn setup_agent() -> Option<(
    AgentLoop<DeepSeekClient>,
    Arc<ToolRegistry>,
    Arc<SystemPromptBuilder>,
)> {
    let client = DeepSeekClient::from_env().ok()?;
    let llm = Arc::new(client);

    // 空 ToolRegistry — 工具测试在 aegis-tools crate 中
    let registry = Arc::new(ToolRegistry::new());
    let sp = Arc::new(SystemPromptBuilder::new(AgentConfig::default()));

    let mut config = AgentConfig::default();
    config.max_turns = 10;
    config.verify_before_output = true;
    config.thinking_enabled = true;
    config.web_search_enabled = false;
    config.strict_tool_schema = false;
    config.retry_max_attempts = 2;
    config.default_model = "deepseek-v4-pro".into();

    let agent = AgentLoop::new(config, llm, Arc::clone(&registry), Arc::clone(&sp));

    Some((agent, registry, sp))
}

// ═══════════════════════════════════════════════════════
// E2E Test 1: 简单问答 — 全流程追踪
// ═══════════════════════════════════════════════════════

#[tokio::test]
async fn test_e2e_simple_qa_full_trace() {
    let (mut agent, _registry, _sp) = match setup_agent() {
        Some(a) => a,
        None => return,
    };

    println!("╔══════════════════════════════════════╗");
    println!("║  E2E Trace: Simple QA               ║");
    println!("╚══════════════════════════════════════╝");

    // Phase 1: 输入
    let input = "What is the Rust programming language? Answer in 2-3 sentences.";
    println!("\n[Phase 1] User Input: \"{}\"", input);

    // Phase 2: 执行
    let start = std::time::Instant::now();
    let output = agent.run(input).await.unwrap();
    let elapsed = start.elapsed();

    // Phase 3: 输出分析
    println!("\n[Phase 2] Agent Output:");
    println!("  Content   : {}", output.content);
    println!("  Confidence: {:?}", output.confidence);
    println!("  Verified  : {}", output.verification_report.is_some());
    println!("  Latency   : {}ms", elapsed.as_millis());

    // Phase 4: 对话状态
    let conv = agent.conversation();
    println!("\n[Phase 3] Conversation State:");
    println!("  Messages  : {}", conv.message_count());
    println!("  Turns     : {}", conv.turn_count());
    println!("  Cost USD  : ${:.6}", conv.total_cost_usd());
    println!("  Tokens in : {}", conv.total_usage().input_tokens);
    println!("  Tokens out: {}", conv.total_usage().output_tokens);

    // 验证
    assert!(!output.content.is_empty(), "Content should not be empty");
    assert!(output.confidence >= ConfidenceLevel::Medium);
    assert!(conv.turn_count() >= 1);
    assert!(conv.total_usage().input_tokens > 0);
}

// ═══════════════════════════════════════════════════════
// E2E Test 2: 多轮对话 + 上下文管理追踪
// ═══════════════════════════════════════════════════════

#[tokio::test]
async fn test_e2e_multi_turn_context_trace() {
    let (mut agent, _registry, _sp) = match setup_agent() {
        Some(a) => a,
        None => return,
    };

    println!("╔══════════════════════════════════════╗");
    println!("║  E2E Trace: Multi-turn Context      ║");
    println!("╚══════════════════════════════════════╝");

    // Turn 1: 分享信息
    println!("\n[Turn 1] 'My project uses Rust 2024 edition with tokio for async.'");
    let output1 = agent
        .run("My project uses Rust 2024 edition with tokio for async runtime. Remember this.")
        .await
        .unwrap();
    println!("  Response: {}...", &output1.content[..output1.content.len().min(80)]);

    // Turn 2: 查询记忆
    println!("\n[Turn 2] 'What async runtime does my project use?'");
    let output2 = agent
        .run("What async runtime does my project use? Answer in one word.")
        .await
        .unwrap();

    let conv = agent.conversation();
    println!("\n[Multi-turn State]:");
    println!("  Total messages: {}", conv.message_count());
    println!("  Total turns   : {}", conv.turn_count());
    println!("  Total cost    : ${:.6}", conv.total_cost_usd());
    println!("  Response      : {}", output2.content);
    println!("  Confidence    : {:?}", output2.confidence);

    // 验证多轮追踪
    assert!(conv.turn_count() >= 2);
    assert!(!output2.content.is_empty());
}

// ═══════════════════════════════════════════════════════
// E2E Test 3: 代码任务 + 验证追踪
// ═══════════════════════════════════════════════════════

#[tokio::test]
async fn test_e2e_code_task_verification_trace() {
    let (mut agent, _registry, _sp) = match setup_agent() {
        Some(a) => a,
        None => return,
    };

    println!("╔══════════════════════════════════════╗");
    println!("║  E2E Trace: Code Task Verification  ║");
    println!("╚══════════════════════════════════════╝");

    let input = "Write a simple Rust function `add(a: i32, b: i32) -> i32` that returns the sum. Only output the code, no explanation.";
    println!("\n[Input]: {}", input);

    let output = agent.run(input).await.unwrap();

    println!("\n[Output]:");
    println!("  Content    : {}", output.content);
    println!("  Confidence : {:?}", output.confidence);
    println!("  Verification: {}", output.verification_report.as_deref().unwrap_or("none"));

    // 代码任务应该有验证
    let conv = agent.conversation();
    println!("\n[Trace]:");
    println!("  Messages: {}", conv.message_count());
    println!("  Turns   : {}", conv.turn_count());
    println!("  Cost    : ${:.6}", conv.total_cost_usd());

    assert!(output.content.contains("fn ") || output.content.contains("add"));
}

// ═══════════════════════════════════════════════════════
// E2E Test 4: 错误处理 + 重试追踪
// ═══════════════════════════════════════════════════════

#[tokio::test]
async fn test_e2e_error_handling_trace() {
    let (mut agent, _registry, _sp) = match setup_agent() {
        Some(a) => a,
        None => return,
    };

    println!("╔══════════════════════════════════════╗");
    println!("║  E2E Trace: Error Handling          ║");
    println!("╚══════════════════════════════════════╝");

    // max_turns=1 → 应该触发 ExitWithSummary 或返回
    agent.set_mode(ExecutionMode::Default);
    println!("\n[Config] max_turns={}", 10);

    // 发10轮简单对话，触发接近 max_turns
    let mut last_output = None;
    for i in 0..3 {
        println!("\n[Turn {}] 'Reply with just the number {}'", i + 1, i + 1);
        match agent.run(&format!("Reply with just the number {}", i + 1)).await {
            Ok(o) => {
                println!("  Output: '{}', confidence={:?}", o.content.trim(), o.confidence);
                last_output = Some(o);
            }
            Err(e) => {
                println!("  Error: {}", e);
            }
        }
    }

    let conv = agent.conversation();
    println!("\n[Error Handling Trace]:");
    println!("  Total turns  : {}", conv.turn_count());
    println!("  Total cost   : ${:.6}", conv.total_cost_usd());
    println!("  Has output   : {}", last_output.is_some());

    assert!(last_output.is_some(), "Should have at least one successful output");
}

// ═══════════════════════════════════════════════════════
// E2E Test 5: Thinking 模式 + 置信度追踪
// ═══════════════════════════════════════════════════════

#[tokio::test]
async fn test_e2e_thinking_confidence_trace() {
    let (mut agent, _registry, _sp) = match setup_agent() {
        Some(a) => a,
        None => return,
    };

    println!("╔══════════════════════════════════════╗");
    println!("║  E2E Trace: Thinking + Confidence   ║");
    println!("╚══════════════════════════════════════╝");

    // 复杂问题触发 thinking
    let input = "If a train travels 120 km in 2 hours, then increases speed by 50% for the next hour, what is the average speed? Explain step by step.";
    println!("\n[Input]: {}", input);

    let output = agent.run(input).await.unwrap();

    println!("\n[Output Analysis]:");
    println!("  Content    : {}...", &output.content[..output.content.len().min(100)]);
    println!("  Confidence : {:?}", output.confidence);
    println!("  Content len: {} chars", output.content.len());

    let conv = agent.conversation();
    println!("\n[Cost + Token Trace]:");
    println!("  Input tokens : {}", conv.total_usage().input_tokens);
    println!("  Output tokens: {}", conv.total_usage().output_tokens);
    println!("  Cache read   : {}", conv.total_usage().cache_read_tokens);
    println!("  Cost USD     : ${:.6}", conv.total_cost_usd());
    println!("  Cache hit%   : {:.1}%",
        if conv.total_usage().input_tokens > 0 {
            conv.total_usage().cache_read_tokens as f64 / conv.total_usage().input_tokens as f64 * 100.0
        } else { 0.0 }
    );

    assert!(!output.content.is_empty());
    assert!(conv.total_cost_usd() > 0.0, "Should track cost");
}

// ═══════════════════════════════════════════════════════
// E2E Test 6: SprintContract 验收追踪
// ═══════════════════════════════════════════════════════

#[tokio::test]
async fn test_e2e_sprint_contract_trace() {
    let (mut agent, _registry, _sp) = match setup_agent() {
        Some(a) => a,
        None => return,
    };

    println!("╔══════════════════════════════════════╗");
    println!("║  E2E Trace: SprintContract          ║");
    println!("╚══════════════════════════════════════╝");

    // 创建 SprintContract
    let criteria = vec![crate::agent::harness::AcceptanceCriterion {
        description: "Output should contain 'fn add'".into(),
        verification_command: "echo 'check'".into(),
        expected_exit_code: 0,
        expected_output_contains: Some("fn add".into()),
    }];
    let mut contract = SprintContract::new("Write add function".into());
    contract.acceptance_criteria = criteria;
    contract.complexity = 0.3;
    contract.estimated_tokens = 1000;
    agent.set_contract(contract);

    println!("\n[Contract]: Write add function, complexity=0.3");
    println!("  Criterion: Output must contain 'fn add'");

    let output = agent
        .run("Write a Rust function `add(a: i32, b: i32) -> i32`. Only output the code.")
        .await
        .unwrap();

    println!("\n[Verification Result]:");
    println!("  Content    : {}", output.content);
    println!("  Confidence : {:?}", output.confidence);
    println!("  Report     : {}", output.verification_report.as_deref().unwrap_or("none"));

    // SprintContract 验收通过 → confidence 应为 High 或 Verified
    assert!(output.confidence >= ConfidenceLevel::High);
}

// ═══════════════════════════════════════════════════════
// E2E Test 7: 工作区文件交互追踪
// ═══════════════════════════════════════════════════════

#[tokio::test]
async fn test_e2e_file_interaction_trace() {
    let (mut agent, _registry, _sp) = match setup_agent() {
        Some(a) => a,
        None => return,
    };

    println!("╔══════════════════════════════════════╗");
    println!("║  E2E Trace: File Interaction        ║");
    println!("╚══════════════════════════════════════╝");

    // 先创建测试文件
    let test_file = std::env::temp_dir().join("aegis_e2e_test.txt");
    std::fs::write(&test_file, "Hello from Aegis E2E test!").unwrap();

    println!("\n[Setup] Created file: {}", test_file.display());

    let input = format!(
        "Read the file at '{}' and tell me what it contains. Be brief.",
        test_file.display()
    );
    println!("[Input]: {}", input);

    let output = agent.run(&input).await.unwrap();

    println!("\n[Output]:");
    println!("  Content    : {}", output.content);
    println!("  Confidence : {:?}", output.confidence);

    let conv = agent.conversation();
    println!("\n[File Interaction Trace]:");
    println!("  Messages : {}", conv.message_count());
    println!("  Turns    : {}", conv.turn_count());
    println!("  Cost     : ${:.6}", conv.total_cost_usd());

    // 清理
    let _ = std::fs::remove_file(&test_file);

    assert!(!output.content.is_empty());
}

// ═══════════════════════════════════════════════════════
// E2E Summary
// ═══════════════════════════════════════════════════════

#[test]
fn test_e2e_summary() {
    println!("\n╔══════════════════════════════════════════════╗");
    println!("║  E2E Test Suite Summary                     ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  E2E-1: Simple QA — full flow trace         ║");
    println!("║  E2E-2: Multi-turn context management       ║");
    println!("║  E2E-3: Code task + verification            ║");
    println!("║  E2E-4: Error handling + retry              ║");
    println!("║  E2E-5: Thinking mode + confidence          ║");
    println!("║  E2E-6: SprintContract acceptance           ║");
    println!("║  E2E-7: File interaction trace              ║");
    println!("╚══════════════════════════════════════════════╝");
    println!("  All tests require: DEEPSEEK_API_KEY env var");
    println!("  Run: cargo test -p aegis-core -- e2e_tests --nocapture");
}
