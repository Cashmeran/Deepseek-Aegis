//! Structured logging and monitoring.
//!
//! Trace spans instrument the critical path for latency/error/cost tracking:
//!   agent.run    → full task execution (session_id, turns, outcome)
//!   llm.chat     → API call (model, tokens, latency_ms, cost)
//!   tool.repair  → repair passes (scavenge_count, storm_suppressed)
//!   tool.execute → tool execution (name, latency_ms, is_error)
//!
//! Usage: `RUST_LOG=aegis_core=debug cargo run`

/// Initialize JSON-structured file logging
pub fn init_tracing() {
    let file_appender = tracing_appender::rolling::daily(".agent/logs", "aegis.log");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "aegis_core=info".into()),
        )
        .with_writer(file_appender)
        .with_ansi(false)
        .json()
        .init();
}

/// Performance measurement helpers for the agent loop
#[derive(Debug, Clone, Default)]
pub struct AgentMetrics {
    pub turns: u32,
    pub llm_calls: u32,
    pub tool_calls: u32,
    pub tool_errors: u32,
    pub repairs_applied: u32,
    pub folds_triggered: u32,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub total_cost_usd: f64,
    pub total_latency_ms: u64,
}

impl AgentMetrics {
    pub fn snapshot(&self) -> Self { self.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_does_not_panic() {
        init_tracing();
    }

    #[test]
    fn test_metrics_default() {
        let m = AgentMetrics::default();
        assert_eq!(m.turns, 0);
        assert_eq!(m.total_cost_usd, 0.0);
    }
}
