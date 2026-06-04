use crate::gating::CraniMemGater;
use crate::store::MemoryStore;
use crate::types::*;
use aegis_core::error::AgentResult;
use sha2::Digest;
use std::sync::Arc;

/// Episode lifecycle manager: open → execute → label → (gate) → close
pub struct EpisodeManager {
    store: Arc<dyn MemoryStore>,
    gater: Arc<CraniMemGater>,
}

impl EpisodeManager {
    pub fn new(store: Arc<dyn MemoryStore>, gater: Arc<CraniMemGater>) -> Self {
        Self { store, gater }
    }

    pub fn open(&self, session_id: &str, user_request: &str) -> AgentResult<MemoryNodeId> {
        self.store.create_episode(session_id, user_request)
    }

    pub fn close(
        &self,
        episode_id: &MemoryNodeId,
        outcome: EpisodeOutcome,
        agent_response: &str,
        error_signature: Option<&str>,
        user_correction: Option<&str>,
    ) -> AgentResult<EpisodeCloseResult> {
        self.store.label_episode(episode_id, outcome, agent_response, error_signature)?;

        if let Some(correction) = user_correction {
            self.store.record_correction(episode_id, agent_response, correction)?;
        }

        let mut admitted = false;
        let mut utility = 0.0f32;

        if outcome == EpisodeOutcome::Failure || user_correction.is_some() {
            if let Some(ep) = self.store.get_episode(episode_id)? {
                let occurrence = self.store.count_similar_patterns(&ep)?;
                let cross_session = self.store.count_cross_session_occurrences(&ep)?;

                let gate_input = GateInput {
                    task_description: ep.user_request.clone(),
                    memory_content: format!(
                        "{} → {}",
                        agent_response,
                        user_correction.unwrap_or("(no correction)")
                    ),
                    memory_type: MemoryNodeType::Episode,
                    occurrence_count: occurrence,
                    cross_session_count: cross_session,
                    corrective: user_correction.is_some(),
                };

                let result = self.gater.evaluate(&gate_input);
                admitted = result.admitted;
                utility = result.utility_score;

                if admitted {
                    // Embedding generation is deferred to the embedding feature (optional)
                    // When enabled, embedder generates 384d vector for retrieval
                }
            }
        }

        Ok(EpisodeCloseResult {
            episode_id: episode_id.clone(),
            admitted,
            utility_score: utility,
        })
    }
}

#[derive(Debug, Clone)]
pub struct EpisodeCloseResult {
    pub episode_id: MemoryNodeId,
    pub admitted: bool,
    pub utility_score: f32,
}

/// Heuristic: is this user message a correction?
pub fn is_user_correction(user_message: &str) -> bool {
    let lower = user_message.to_lowercase();
    let negation = [
        "no", "不对", "错了", "wrong", "incorrect", "not right", "don't",
        "doesn't work", "should be", "应该是", "改成",
    ];
    let correction = [
        "fix", "修复", "change", "修改", "instead", "替换", "rather",
    ];
    negation.iter().any(|n| lower.contains(n))
        || correction.iter().any(|c| lower.starts_with(c) || lower.contains(&format!(" {}", c)))
}

/// Compute stable error signature from error output for dedup
pub fn compute_error_signature(error_output: &str) -> String {
    let normalized = error_output
        .lines()
        .filter(|l| l.contains("error") || l.contains("Error") || l.contains("panic"))
        .take(3)
        .collect::<Vec<_>>()
        .join("\n");
    format!("{:x}", sha2::Sha256::new().chain_update(normalized.as_bytes()).finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correction_detection() {
        assert!(is_user_correction("no, that's wrong"));
        assert!(is_user_correction("不对，应该改成这样"));
        assert!(is_user_correction("should be different"));
        assert!(!is_user_correction("what does this do?"));
        assert!(!is_user_correction("looks great, thanks!"));
    }
}
