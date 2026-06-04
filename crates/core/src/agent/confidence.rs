/// 推理树的结构特征。从 LLM 的 CoT (Chain-of-Thought) 轨迹中提取。
/// 参考 Playing Psychic (arXiv:2604.16931) 的特征设计。
#[derive(Debug, Clone, Default)]
pub struct ThoughtTreeFeatures {
    /// 推理的最大深度 (嵌套层次), >15层 → 可能迷路了
    pub max_depth: usize,
    /// 总推理步骤数, >50步 → 可能过度思考
    pub total_steps: usize,
    /// 回溯次数 (LLM 纠正自己), >5次 → 高度不确定
    pub backtrack_count: usize,
    /// 分支因子 (同时考虑了几个方案), >3 → 探索太多, 不确定
    pub branch_factor: f32,
    /// 自相矛盾次数 (前后说法不一致), >2 → 推理质量低
    pub contradiction_count: usize,
    /// 代码提及频率 (生成的代码中引用外部文件/函数的次数)
    pub code_reference_count: usize,
    /// 不确定性词汇频率 ("might", "could", "possibly", "maybe")
    pub uncertainty_word_count: usize,
}

/// 逻辑回归权重。可从历史数据拟合，也可专家手工设定。
#[derive(Debug, Clone)]
pub struct ConfidenceWeights {
    pub depth_penalty: f32,             // -0.05 per depth level beyond 10
    pub step_penalty: f32,              // -0.02 per step beyond 30
    pub backtrack_penalty: f32,         // -0.15 per backtrack
    pub branch_penalty: f32,            // -0.10 per extra branch
    pub contradiction_penalty: f32,     // -0.20 per contradiction (最严重)
    pub uncertainty_word_penalty: f32,  // -0.02 per uncertainty word
}

impl Default for ConfidenceWeights {
    fn default() -> Self {
        Self {
            depth_penalty: 0.05,
            step_penalty: 0.02,
            backtrack_penalty: 0.15,
            branch_penalty: 0.10,
            contradiction_penalty: 0.20,
            uncertainty_word_penalty: 0.02,
        }
    }
}

/// 信心评分器。基于推理树结构特征，不依赖语义内容 (无需额外 LLM 调用)。
pub struct ConfidenceScorer {
    weights: ConfidenceWeights,
}

impl ConfidenceScorer {
    /// 创建使用默认权重的评分器。
    pub fn new() -> Self {
        Self {
            weights: ConfidenceWeights::default(),
        }
    }

    /// 创建使用自定义权重的评分器。
    pub fn with_weights(weights: ConfidenceWeights) -> Self {
        Self { weights }
    }

    /// 从 LLM 的 CoT 文本中提取结构特征，计算信心分数。
    /// 空或极短 CoT → 返回 0.5 (中性，非 HIGH 非 LOW)。
    pub fn score(&self, cot_text: &str) -> f32 {
        if cot_text.trim().len() < 20 {
            return 0.5;
        }

        let features = self.extract_features(cot_text);
        let mut score = 1.0_f32;

        // 逐项扣分，每项有封顶防止单项拉低总分
        score -= (features
            .max_depth
            .saturating_sub(10) as f32
            * self.weights.depth_penalty)
            .min(0.3);
        score -= (features
            .total_steps
            .saturating_sub(30) as f32
            * self.weights.step_penalty)
            .min(0.2);
        score -= (features.backtrack_count as f32 * self.weights.backtrack_penalty).min(0.4);
        score -= ((features.branch_factor - 2.0).max(0.0) * self.weights.branch_penalty).min(0.3);
        score -= (features.contradiction_count as f32 * self.weights.contradiction_penalty)
            .min(0.5);
        score -= (features.uncertainty_word_count as f32 * self.weights.uncertainty_word_penalty)
            .min(0.1);

        score.clamp(0.0, 1.0)
    }

    /// 从 CoT 文本中提取结构特征。
    /// 使用简单的文本标记解析推理结构。
    fn extract_features(&self, cot_text: &str) -> ThoughtTreeFeatures {
        let lower = cot_text.to_lowercase();

        // 步骤计数: 标题行 "### Step" 或行首 "Step N:"
        let total_steps = cot_text
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                t.starts_with("### Step")
                    || t.starts_with("Step ")
                    || t.starts_with("step ")
                    || (t.starts_with("- ") && t.len() > 20) // 长 bullet 可能是一步
            })
            .count();

        let backtrack_count = lower.matches("however").count()
            + lower.matches("but actually").count()
            + lower.matches("wait").count()
            + lower.matches("let me reconsider").count()
            + lower.matches("actually").count() / 3; // "actually" 太多才算回溯

        let contradiction_count = lower.matches("i said").count()
            + lower.matches("contradicts").count()
            + lower.matches("on second thought").count()
            + lower.matches("that can't be right").count();

        let uncertainty_word_count = lower.matches("might").count()
            + lower.matches("could be").count()
            + lower.matches("possibly").count()
            + lower.matches("maybe").count()
            + lower.matches("unsure").count()
            + lower.matches("probably").count();

        let branch_count = lower.matches("option").count()
            + lower.matches("alternative").count()
            + lower.matches("approach").count();

        let branch_factor = if total_steps > 0 {
            branch_count as f32 / total_steps.max(1) as f32
        } else {
            0.0
        };

        // 嵌套深度: 统计每一行的前导空格数，取最大缩进层级
        let max_depth = cot_text
            .lines()
            .map(|l| {
                let trimmed = l.trim_start();
                if trimmed.is_empty() {
                    0
                } else {
                    l.len() - trimmed.len() // 前导空格数
                }
            })
            .max()
            .unwrap_or(0)
            / 2; // 每2空格 = 1缩进层级

        let code_reference_count =
            lower.matches("src/").count() + lower.matches("crates/").count();

        ThoughtTreeFeatures {
            max_depth,
            total_steps: total_steps.max(1),
            backtrack_count,
            branch_factor,
            contradiction_count,
            code_reference_count,
            uncertainty_word_count,
        }
    }
}

impl Default for ConfidenceScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_cot_returns_neutral() {
        let scorer = ConfidenceScorer::new();
        assert!((scorer.score("") - 0.5).abs() < 0.01);
        assert!((scorer.score("short") - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_clean_cot_returns_high() {
        let scorer = ConfidenceScorer::new();
        let clean = "Step 1: Parse input.\nStep 2: Validate.\nStep 3: Return result.\nThis approach is correct.";
        let score = scorer.score(clean);
        assert!(score > 0.85, "Expected high confidence, got {}", score);
    }

    #[test]
    fn test_contradiction_reduces_score() {
        let scorer = ConfidenceScorer::new();
        let contradicting = "I said we should use HashMap. But that can't be right. On second thought, BTreeMap is better. However, wait, let me reconsider...";
        let score = scorer.score(contradicting);
        assert!(score < 0.7, "Expected reduced confidence, got {}", score);
    }

    #[test]
    fn test_uncertainty_reduces_score() {
        let scorer = ConfidenceScorer::new();
        // 5 uncertainty words × 0.02 = 0.10 penalty, score ≈ 0.90
        let uncertain = "We might use Option. Could be Result. Possibly we could throw an error. I'm unsure which approach is best. Maybe we should try both.";
        let score = scorer.score(uncertain);
        assert!(score < 0.92, "Expected slight reduction, got {}", score);
        assert!(score > 0.85, "Expected near 0.90, got {}", score);
    }

    #[test]
    fn test_deep_nesting_reduces_score() {
        let scorer = ConfidenceScorer::new();
        // 模拟深度嵌套推理: 每层增加 2 空格缩进
        let mut lines = vec!["Step 1: Top-level analysis".to_string()];
        for i in 1..12 {
            lines.push(format!("{}  - Sub-step {}: deeper reasoning layer", "  ".repeat(i), i));
        }
        let deep = lines.join("\n");
        let score = scorer.score(&deep);
        // total_steps=12, max_depth≈22/2=11 → >10 → 1*0.05=0.05 penalty, score≈0.95
        assert!(score >= 0.90, "Expected mild reduction for depth, got {}", score);
    }

    #[test]
    fn test_custom_weights() {
        let weights = ConfidenceWeights {
            contradiction_penalty: 0.5, // 更严格
            ..Default::default()
        };
        let scorer = ConfidenceScorer::with_weights(weights);
        let contradicting = "I said X. That contradicts my earlier statement. I said Y instead. Actually wait, reconsider.";
        let score = scorer.score(contradicting);
        assert!(score < 0.5, "Expected very low with strict weights, got {}", score);
    }

    #[test]
    fn test_score_clamped_to_zero() {
        let scorer = ConfidenceScorer::new();
        // 极端信号：大量矛盾 + 回溯 + 不确定
        let terrible = "wait however but actually".repeat(20)
            + &"contradicts i said on second thought that can't be right".repeat(20)
            + &"might could be possibly maybe unsure probably".repeat(20);
        let score = scorer.score(&terrible);
        assert!(score >= 0.0, "Score should not be negative: {}", score);
        assert!(score <= 1.0, "Score should not exceed 1.0: {}", score);
    }
}
