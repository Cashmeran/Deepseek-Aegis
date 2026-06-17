use crate::gating::CraniMemGater;
use crate::store::MemoryStore;
use crate::types::*;
use aegis_core::error::AgentResult;
use sha2::Digest;
use std::sync::Arc;

/// Extract a human-readable error message from the agent's response.
fn extract_error_message(agent_response: &str, error_signature: &str) -> String {
    // Try to find error lines in the response
    for line in agent_response.lines() {
        let lower = line.to_lowercase();
        if lower.contains("error") || lower.contains("panic") || lower.contains("fail") {
            let msg = line.trim().to_string();
            if msg.len() > 10 && msg.len() < 500 {
                return msg;
            }
        }
    }
    // Fallback: use the signature itself
    format!("error signature: {}", &error_signature[..error_signature.len().min(64)])
}

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

        if (outcome == EpisodeOutcome::Failure || user_correction.is_some())
            && let Some(ep) = self.store.get_episode(episode_id)? {
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
                    // Auto-record Bug on failure
                    if outcome == EpisodeOutcome::Failure {
                        let error_sig_str = error_signature.map(|s| s.to_string());
                        let error_msg = error_sig_str.as_deref()
                            .map(|_s| extract_error_message(agent_response, _s))
                            .unwrap_or_else(|| "Unknown error".into());
                        let stack_hash = error_sig_str.clone().unwrap_or_else(|| {
                            format!("{:x}", sha2::Sha256::new().chain_update(agent_response.as_bytes()).finalize())
                        });
                        let bug = Bug {
                            id: make_memory_id(&error_msg, "Bug", chrono::Utc::now().timestamp()),
                            description: format!("{} → {}", ep.user_request, &error_msg[..error_msg.len().min(200)]),
                            stack_trace_hash: stack_hash.clone(),
                            error_message: error_msg,
                            file_path: ep.files_modified.first().cloned(),
                            line_number: None,
                            severity: BugSeverity::Medium,
                            occurrence_count: 1,
                            first_seen_at: chrono::Utc::now(),
                            last_seen_at: chrono::Utc::now(),
                            metadata: serde_json::json!({"episode_id": ep.id}),
                        };
                        let _ = self.store.record_bug(&bug);
                        tracing::info!(bug_id = %bug.id, error = %bug.error_message.chars().take(80).collect::<String>(), "memory: bug recorded");
                    }

                    // Auto-record Fix on success (if there was a prior failure in this session)
                    if outcome == EpisodeOutcome::Success && user_correction.is_some() {
                        let fix_desc = format!(
                            "Correction applied: {} → {}",
                            &agent_response[..agent_response.len().min(100)],
                            user_correction.unwrap_or("")
                        );
                        let fix = Fix {
                            id: make_memory_id(&fix_desc, "Fix", chrono::Utc::now().timestamp()),
                            description: fix_desc,
                            strategy: FixStrategy::Other,
                            file_changes: ep.files_modified.iter().map(|f| FileChange {
                                file_path: f.clone(), old_string: String::new(), new_string: String::new(), change_type: ChangeType::Modify,
                            }).collect(),
                            verification_command: None,
                            is_successful: true,
                            success_count: 1,
                            failure_count: 0,
                            created_at: chrono::Utc::now(),
                            metadata: serde_json::json!({"episode_id": ep.id}),
                        };
                        // Link Fix to the most recent Bug in this session
                        if let Ok(bugs) = self.store.find_bugs_by_signature(&ep.error_signature.clone().unwrap_or_default()) {
                            if let Some(bug) = bugs.first() {
                                let _ = self.store.record_fix(&fix, &bug.id);
                                tracing::info!(fix_id = %fix.id, bug_id = %bug.id, "memory: fix recorded");
                            }
                        } else {
                            let _ = self.store.record_fix_no_bug(&fix);
                        }
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
