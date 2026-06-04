use crate::types::{GateInput, GateResult, MemoryNodeType};

/// CraniMem goal-conditioned gating — 6-dimension utility scoring
pub struct CraniMemGater {
    admission_threshold: f32,
}

impl CraniMemGater {
    pub fn new(threshold: f32) -> Self {
        Self { admission_threshold: threshold }
    }

    pub fn evaluate(&self, input: &GateInput) -> GateResult {
        let corrective_bonus = if input.corrective { 0.3 } else { 0.0 };
        let frequency_score = (input.occurrence_count as f32 * 0.1).min(0.3);
        let cross_session_score = (input.cross_session_count as f32 * 0.15).min(0.3);
        let type_importance = match input.memory_type {
            MemoryNodeType::Bug | MemoryNodeType::Fix | MemoryNodeType::RootCause => 0.3,
            MemoryNodeType::Insight => 0.2,
            MemoryNodeType::Preference => 0.25,
            MemoryNodeType::Episode => 0.1,
        };
        let specificity = (input.memory_content.len() as f32 / 1000.0).min(1.0);
        let specificity_score = if (0.3..0.8).contains(&specificity) { 0.2 } else { 0.0 };

        let utility = (corrective_bonus + frequency_score + cross_session_score + type_importance + specificity_score).clamp(0.0, 1.0);
        let admitted = utility >= self.admission_threshold;

        GateResult {
            admitted,
            utility_score: utility,
            reason: format!(
                "utility={:.2} threshold={:.1} (corrective={:.1} freq={:.1} cross_session={:.1} type={:.1} spec={:.1})",
                utility, self.admission_threshold, corrective_bonus, frequency_score, cross_session_score, type_importance, specificity_score,
            ),
        }
    }
}

impl Default for CraniMemGater {
    fn default() -> Self {
        Self::new(0.6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(corrective: bool, occurrence: u32, cross: u32, mt: MemoryNodeType) -> GateInput {
        GateInput {
            task_description: "test task".into(),
            memory_content: "test content that is moderately specific and long enough".into(),
            memory_type: mt,
            occurrence_count: occurrence,
            cross_session_count: cross,
            corrective,
        }
    }

    #[test]
    fn test_corrective_admitted() {
        let g = CraniMemGater::default();
        let r = g.evaluate(&make_input(true, 3, 2, MemoryNodeType::Bug));
        assert!(r.admitted);
        assert!(r.utility_score > 0.6);
    }

    #[test]
    fn test_low_occurrence_rejected() {
        let g = CraniMemGater::default();
        let r = g.evaluate(&make_input(false, 0, 0, MemoryNodeType::Episode));
        assert!(!r.admitted);
    }

    #[test]
    fn test_cross_session_boosts_score() {
        let g = CraniMemGater::default();
        let r_low = g.evaluate(&make_input(false, 5, 0, MemoryNodeType::Bug));
        let r_high = g.evaluate(&make_input(false, 5, 3, MemoryNodeType::Bug));
        assert!(r_high.utility_score > r_low.utility_score);
    }
}
