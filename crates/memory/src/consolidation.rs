use crate::store::MemoryStore;
use crate::types::*;
use aegis_core::error::AgentResult;
use chrono::Utc;
use sha2::Digest;
use std::sync::Arc;

/// Dream consolidator — 3-gate trigger, 3-phase execution
pub struct DreamConsolidator {
    store: Arc<dyn MemoryStore>,
    config: ConsolidationConfig,
}

#[derive(Debug, Clone)]
pub struct ConsolidationConfig {
    pub min_interval_hours: u32,
    pub min_new_sessions: u32,
    pub min_episodes_for_insight: u32,
    pub pruning_utility_threshold: f32,
    pub pruning_max_age_days: u32,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            min_interval_hours: 12,
            min_new_sessions: 3,
            min_episodes_for_insight: 2,
            pruning_utility_threshold: 0.3,
            pruning_max_age_days: 90,
        }
    }
}

impl DreamConsolidator {
    pub fn new(store: Arc<dyn MemoryStore>, config: ConsolidationConfig) -> Self {
        Self { store, config }
    }

    /// Check 3-gate trigger
    pub fn should_consolidate(&self) -> AgentResult<bool> {
        let state = self.store.get_consolidation_state()?;
        if matches!(state, ConsolidationState::Running { .. }) {
            return Ok(false); // G3: lock gate
        }
        let pending = self.store.get_pending_consolidation_episodes(
            self.config.min_new_sessions,
            self.config.min_interval_hours,
        )?;
        Ok(pending.len() >= self.config.min_episodes_for_insight as usize)
    }

    /// Execute consolidation (call from tokio::spawn)
    pub fn consolidate(&self) -> AgentResult<ConsolidationResult> {
        self.store.set_consolidation_state(ConsolidationState::Running {
            started_at: Utc::now(),
        })?;

        let result = self.do_consolidate();
        match &result {
            Ok(r) => {
                self.store.set_consolidation_state(ConsolidationState::Completed {
                    insights_generated: r.insights_generated,
                    pruned_count: r.pruned_count,
                })?;
            }
            Err(e) => {
                self.store.set_consolidation_state(ConsolidationState::Failed {
                    error: e.to_string(),
                })?;
            }
        }
        result
    }

    fn do_consolidate(&self) -> AgentResult<ConsolidationResult> {
        let mut insights_generated = 0u32;

        // P1: Aggregate — collect candidate episodes
        let episodes = self.store.get_pending_consolidation_episodes(
            self.config.min_new_sessions,
            self.config.min_interval_hours,
        )?;

        // P2: Generate — detect cross-session patterns and create real Insights
        let mut seen_sigs: std::collections::HashMap<String, Vec<&crate::types::Episode>> = std::collections::HashMap::new();
        for ep in &episodes {
            if ep.outcome == EpisodeOutcome::Failure {
                if let Some(ref err_sig) = ep.error_signature {
                    seen_sigs.entry(err_sig.clone()).or_default().push(ep);
                }
            }
        }

        for (sig, eps) in &seen_sigs {
            if eps.len() >= 2 {
                // Multi-session same error → generate consolidated insight
                let descriptions: Vec<&str> = eps.iter()
                    .filter_map(|e| e.user_request.lines().next())
                    .collect();
                let insight_content = format!(
                    "Recurring error pattern ({} occurrences across {} sessions):\n  Error signature: {}\n  First seen: {}\n  Sample causes: {}",
                    eps.len(),
                    eps.iter().filter_map(|e| Some(&e.session_id)).collect::<std::collections::HashSet<_>>().len(),
                    &sig[..sig.len().min(16)],
                    eps.iter().map(|e| e.created_at).min().map(|t| t.to_string()).unwrap_or_default(),
                    descriptions.join("; "),
                );
                let ep_ids: Vec<String> = eps.iter().map(|e| e.id.clone()).collect();
                let insight = crate::types::Insight {
                    id: format!("{:x}", sha2::Sha256::new().chain_update(insight_content.as_bytes()).finalize()),
                    content: insight_content,
                    confidence: 0.7,
                    utility_score: 0.6,
                    version: 1,
                    source_count: eps.len() as u32,
                    last_activated_at: chrono::Utc::now(),
                    created_at: chrono::Utc::now(),
                    status: crate::types::InsightStatus::Stable,
                    metadata: serde_json::json!({"error_signature": sig}),
                };
                if self.store.upsert_insight(&insight, &ep_ids).is_ok() {
                    insights_generated += 1;
                }
            }
        }

        // P3: Prune — remove stale low-utility insights AND old episodes
        let pruned_insights = self.store.prune_insights(
            self.config.pruning_utility_threshold,
            self.config.pruning_max_age_days,
        )?;
        // Also prune episodes older than 90 days with no outgoing edges
        let pruned_eps = self.store.prune_episodes(90)?;

        Ok(ConsolidationResult { insights_generated, pruned_count: pruned_insights + pruned_eps })
    }
}

#[derive(Debug, Clone)]
pub struct ConsolidationResult {
    pub insights_generated: u32,
    pub pruned_count: u32,
}
