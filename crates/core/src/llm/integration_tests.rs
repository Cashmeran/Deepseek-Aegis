//! Full-system integration tests with real DeepSeek API and all registered tools.
//! Requires DEEPSEEK_API_KEY environment variable.
//! Run: export DEEPSEEK_API_KEY=sk-... && cargo test -p aegis-core -- integration --nocapture

use crate::agent::{AgentLoop, ConfidenceLevel, SystemPromptBuilder};
use crate::llm::deepseek::DeepSeekClient;
use crate::tool_system::{ToolRegistry, ToolCallRepair};
use crate::types::config::AgentConfig;
use crate::types::tool::ExecutionMode;
use std::sync::Arc;

fn setup() -> Option<AgentLoop<DeepSeekClient>> {
    let client = DeepSeekClient::from_env().ok()?;
    let llm = Arc::new(client);

    let registry = Arc::new(ToolRegistry::new());
    // Real tools would be registered here in full integration:
    // registry.register(Arc::new(BashTool::new()));
    // registry.register(Arc::new(FileReadTool::new()));
    // etc.

    let sp = Arc::new(SystemPromptBuilder::new(AgentConfig::default()));
    let mut config = AgentConfig::default();
    config.max_turns = 15;
    config.verify_before_output = true;
    config.thinking_enabled = true;
    config.web_search_enabled = false;

    Some(AgentLoop::new(config, llm, registry, sp))
}

// ═══════════════════════════════════════════════════════════
// Test 1: Full task pipeline — code generation
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integration_code_generation() {
    let mut agent = match setup() { Some(a) => a, None => return };

    let input = "Write a Rust function that checks if a string is a palindrome. \
                 Include a doc comment and a test. Return ONLY the code, no explanation.";

    println!("╔══════════════════════════════════════╗");
    println!("║  Integration: Code Generation       ║");
    println!("╚══════════════════════════════════════╝");
    println!("Input: {}", input);

    let start = std::time::Instant::now();
    let output = agent.run(input).await.unwrap();
    let elapsed = start.elapsed();

    println!("\nOutput: {:.200}", output.content);
    println!("Confidence: {:?}", output.confidence);
    println!("Latency: {}ms", elapsed.as_millis());

    let conv = agent.conversation();
    println!("\nCost: ${:.6}", conv.total_cost_usd());
    println!("Turns: {}", conv.turn_count());
    println!("Tokens in: {} | out: {}", conv.total_usage().input_tokens, conv.total_usage().output_tokens);
    println!("Messages: {}", conv.message_count());

    assert!(!output.content.is_empty());
    assert!(output.confidence >= ConfidenceLevel::Medium);
    assert!(conv.turn_count() >= 1);
    assert!(output.content.contains("fn ") || output.content.contains("def "));
    assert!(conv.total_usage().output_tokens > 0);
}

// ═══════════════════════════════════════════════════════════
// Test 2: Multi-turn conversation with context
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integration_multi_turn() {
    let mut agent = match setup() { Some(a) => a, None => return };

    println!("╔══════════════════════════════════════╗");
    println!("║  Integration: Multi-turn Context    ║");
    println!("╚══════════════════════════════════════╝");

    // Turn 1
    let r1 = agent.run("What is the Rust Result type? Answer in 1 sentence.").await.unwrap();
    println!("Turn 1: {:.100}...", r1.content);
    assert!(!r1.content.is_empty());

    // Turn 2 (references Turn 1 implicitly)
    let r2 = agent.run("Give me a code example of using it.").await.unwrap();
    println!("Turn 2: {:.100}...", r2.content);
    assert!(r2.content.contains("Result") || r2.content.contains("Ok") || r2.content.contains("Err"));

    let conv = agent.conversation();
    println!("\nTurns: {} | Messages: {} | Cost: ${:.6}",
        conv.turn_count(), conv.message_count(), conv.total_cost_usd());
    assert!(conv.turn_count() >= 2);
}

// ═══════════════════════════════════════════════════════════
// Test 3: System prompt self-awareness
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integration_self_awareness() {
    let mut agent = match setup() { Some(a) => a, None => return };

    println!("╔══════════════════════════════════════╗");
    println!("║  Integration: Self-Awareness        ║");
    println!("╚══════════════════════════════════════╝");

    let r = agent.run("What model are you running on? Answer with just the model name.").await.unwrap();
    println!("Response: {}", r.content);
    // Should mention DeepSeek or V4
    assert!(r.content.to_lowercase().contains("deepseek") || r.content.to_lowercase().contains("v4"));
}

// ═══════════════════════════════════════════════════════════
// Test 4: Thinking quality — reasoning traces
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integration_thinking_quality() {
    let mut agent = match setup() { Some(a) => a, None => return };

    println!("╔══════════════════════════════════════╗");
    println!("║  Integration: Thinking Quality      ║");
    println!("╚══════════════════════════════════════╝");

    let input = "Explain step by step: if a Rust Vec has 10 elements and I call .remove(3), \
                 what is the resulting length and what happens to element indices?";
    let r = agent.run(input).await.unwrap();
    println!("Response length: {} chars", r.content.len());
    println!("Confidence: {:?}", r.confidence);
    assert!(r.content.len() > 50, "Expected detailed thinking output");
}

// ═══════════════════════════════════════════════════════════
// Test 5: Error handling — nonsense input
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integration_edge_cases() {
    let mut agent = match setup() { Some(a) => a, None => return };

    println!("╔══════════════════════════════════════╗");
    println!("║  Integration: Edge Cases            ║");
    println!("╚══════════════════════════════════════╝");

    // Empty-ish input
    let r1 = agent.run("hi").await.unwrap();
    println!("Empty-ish: {:.100}", r1.content);
    assert!(!r1.content.is_empty());

    // Very long input (shouldn't crash)
    let long = format!("Explain in one word: {}", "test ".repeat(50));
    let r2 = agent.run(&long).await.unwrap();
    println!("Long input: {:.50}...", r2.content);
    assert!(!r2.content.is_empty());
}

// ═══════════════════════════════════════════════════════════
// Test 6: Prompt instruction following
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integration_instruction_following() {
    let mut agent = match setup() { Some(a) => a, None => return };

    println!("╔══════════════════════════════════════╗");
    println!("║  Integration: Instruction Following ║");
    println!("╚══════════════════════════════════════╝");

    // Test: output format compliance (no emojis, concise)
    let r = agent.run("Say 'hello world' with no extra words, no emojis, no markdown.").await.unwrap();
    println!("Response: '{}'", r.content);
    assert!(!r.content.contains('*'));
    assert!(!r.content.contains("```"));
    assert!(r.content.to_lowercase().contains("hello"));
}

// ═══════════════════════════════════════════════════════════
// Test 7: Cost efficiency benchmark
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integration_cost_efficiency() {
    let mut agent = match setup() { Some(a) => a, None => return };

    println!("╔══════════════════════════════════════╗");
    println!("║  Integration: Cost Efficiency       ║");
    println!("╚══════════════════════════════════════╝");

    let start_cost = agent.conversation().total_cost_usd();
    let r = agent.run("Count from 1 to 3. Just the numbers, no explanation.").await.unwrap();
    let end_cost = agent.conversation().total_cost_usd();
    let task_cost = end_cost - start_cost;

    println!("Task cost: ${:.6}", task_cost);
    println!("Response: {}", r.content);

    // A simple counting task should cost < $0.001
    assert!(task_cost < 0.001, "Simple task too expensive: ${:.6}", task_cost);
    assert!(!r.content.is_empty());
}

// ═══════════════════════════════════════════════════════════
// Run summary
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integration_summary() {
    let mut agent = match setup() { Some(a) => a, None => return };

    println!("╔══════════════════════════════════════╗");
    println!("║  Integration: Final Summary         ║");
    println!("╚══════════════════════════════════════╝");

    let tasks = [
        "What is 2+2?",
        "Name one Rust web framework.",
        "What's the file extension for Rust source files?",
    ];

    let mut total_cost = 0.0;
    for (i, task) in tasks.iter().enumerate() {
        let start = agent.conversation().total_cost_usd();
        let r = agent.run(task).await.unwrap();
        total_cost += agent.conversation().total_cost_usd() - start;
        println!("  Task {}: {} → {:.60}... (${:.6})", i+1, task, r.content, agent.conversation().total_cost_usd() - start);
        assert!(!r.content.is_empty());
    }

    let conv = agent.conversation();
    println!("\nSession summary:");
    println!("  Total turns: {}", conv.turn_count());
    println!("  Total cost: ${:.6}", conv.total_cost_usd());
    println!("  Total tokens in: {} | out: {}", conv.total_usage().input_tokens, conv.total_usage().output_tokens);
    assert!(conv.turn_count() >= 3);
}
