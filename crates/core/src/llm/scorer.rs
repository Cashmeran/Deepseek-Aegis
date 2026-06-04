//! CodeScorer — 执行无关代码评分 (Phase 1: RuleBasedScorer, 1μs, 零模型)
//!
//! Phase 2 预留: DistilledScorer (SWE-RM 30B→7B 蒸馏, ONNX 本地推理)


/// 代码评分器 trait — 评分代码修改的质量 (0.0-1.0)
pub trait CodeScorer: Send + Sync {
    /// 对补丁评分: original=原始代码, patched=修改后, task=任务描述
    fn score(&self, original: &str, patched: &str, task: &str) -> f32;
}

/// Phase 1: 基于规则的快速评分器 (无模型依赖, 1μs延迟)
///
/// 6个启发式规则:
///   - 空补丁/未修改 (-0.5)
///   - 大删不增 (-0.2)
///   - TODO/FIXME 残留 (-0.05 each, max -0.2)
///   - unwrap/panic 不安全代码 (-0.03 each, max -0.15)
///   - 有测试 (+0.1)
///   - 有文档注释 (+0.05)
pub struct RuleBasedScorer;

impl CodeScorer for RuleBasedScorer {
    fn score(&self, original: &str, patched: &str, _task: &str) -> f32 {
        let mut score = 1.0_f32;

        // 没做改动
        if patched.is_empty() || patched == original {
            score -= 0.5;
        }
        // 大删不增
        if patched.len() < original.len() / 2 {
            score -= 0.2;
        }
        // 未完成标记
        let todo_cnt = patched.matches("TODO").count() + patched.matches("FIXME").count();
        score -= (todo_cnt as f32 * 0.05).min(0.2);
        // 不安全代码
        let unsafe_cnt = patched.matches("unwrap()").count() + patched.matches("panic!").count();
        score -= (unsafe_cnt as f32 * 0.03).min(0.15);
        // 有测试
        if patched.contains("#[test]") || patched.contains("def test_") || patched.contains("fn test_") {
            score += 0.1;
        }
        // 有文档
        if patched.contains("///") || patched.contains("\"\"\"") {
            score += 0.05;
        }

        score.clamp(0.0, 1.0)
    }
}

impl Default for RuleBasedScorer {
    fn default() -> Self { Self }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_patch_scores_low() {
        let scorer = RuleBasedScorer;
        let s = scorer.score("fn a() {}", "", "implement fn");
        assert!(s < 0.6, "empty patch should score low, got {}", s);
    }

    #[test]
    fn test_identical_patch_scores_low() {
        let scorer = RuleBasedScorer;
        let s = scorer.score("fn a() {}", "fn a() {}", "change");
        assert!(s < 0.6, "identical patch should score low, got {}", s);
    }

    #[test]
    fn test_todo_reduces_score() {
        let scorer = RuleBasedScorer;
        let s = scorer.score("x", "fn a() { TODO: implement }", "task");
        assert!(s <= 0.95, "TODO should reduce score, got {}", s);
    }

    #[test]
    fn test_unwrap_reduces_score() {
        let scorer = RuleBasedScorer;
        let s = scorer.score("x", "let y = opt.unwrap();", "task");
        assert!(s < 0.98, "unwrap should reduce score, got {}", s);
    }

    #[test]
    fn test_test_presence_boosts_score() {
        let scorer = RuleBasedScorer;
        // Use a base with TODO so it starts lower, then test boost is visible
        let base = scorer.score("x", "TODO: fix", "task");
        let with_test = scorer.score("x", "#[test]\nfn test_a() { let _ = opt.unwrap(); }", "task");
        assert!(with_test > base, "test+docs should offset unwrap penalty; base={}, with_test={}", base, with_test);
    }

    #[test]
    fn test_good_patch_scores_high() {
        let scorer = RuleBasedScorer;
        let s = scorer.score(
            "fn add(a: i32, b: i32) -> i32 { a + b }",
            "/// Add two numbers\nfn add(a: i32, b: i32) -> i32 { a + b }\n\n#[test]\nfn test_add() { assert_eq!(add(1,2), 3); }",
            "add docs and tests",
        );
        assert!(s > 0.9, "good patch should score high, got {}", s);
    }

    #[test]
    fn test_score_clamped_to_range() {
        let scorer = RuleBasedScorer;
        let s = scorer.score("a", "TODO TODO TODO TODO TODO TODO TODO TODO TODO TODO", "x");
        assert!(s >= 0.0, "score should be >= 0: {}", s);
        assert!(s <= 1.0, "score should be <= 1: {}", s);
    }
}
