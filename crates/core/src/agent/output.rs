use serde::{Deserialize, Serialize};

/// Agent 完成一次任务后的输出。
#[derive(Debug, Clone)]
pub struct AgentOutput {
    /// Agent 的最终回复内容
    pub content: String,
    /// 置信度 (来自 Evaluator 的验证结果)
    pub confidence: ConfidenceLevel,
    /// 验证报告 (仅代码任务有)
    pub verification_report: Option<String>,
    /// 执行摘要: "Fixed 3 bugs in auth.rs" 等
    pub summary: Option<String>,
}

/// 置信度级别。驱动 Agent 的验证策略和用户提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    /// 不可用 — 未经过任何验证
    Unknown = 0,
    /// 低置信度 — 需要人工审查
    Low = 1,
    /// 中置信度 — 可能存在问题
    Medium = 2,
    /// 高置信度 — 经过验证
    High = 3,
    /// 已验证 — 经过 Evaluator + 测试双重验证
    Verified = 4,
}

/// 验证结果。由 Evaluator 或 SprintContract 执行后返回。
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// 验证是否通过
    pub passed: bool,
    /// 阻塞性问题数 (不修复无法合入)
    pub blocking_issues: u32,
    /// 建议性问题数 (不影响合入)
    pub advisory_issues: u32,
    /// 验证细节描述
    pub details: String,
    /// 验证报告全文
    pub report: String,
    /// 置信度
    pub confidence: ConfidenceLevel,
}

impl VerificationResult {
    /// 创建通过的验证结果。
    pub fn passed(report: String) -> Self {
        Self {
            passed: true,
            blocking_issues: 0,
            advisory_issues: 0,
            details: String::new(),
            report,
            confidence: ConfidenceLevel::High,
        }
    }

    /// 创建失败的验证结果。
    pub fn failed(blocking: u32, advisory: u32, details: String, report: String) -> Self {
        Self {
            passed: false,
            blocking_issues: blocking,
            advisory_issues: advisory,
            details,
            report,
            confidence: ConfidenceLevel::Low,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_ordering() {
        assert!(ConfidenceLevel::High > ConfidenceLevel::Medium);
        assert!(ConfidenceLevel::Verified > ConfidenceLevel::High);
        assert!(ConfidenceLevel::Low > ConfidenceLevel::Unknown);
    }

    #[test]
    fn test_verification_passed() {
        let v = VerificationResult::passed("All tests passed".into());
        assert!(v.passed);
        assert_eq!(v.blocking_issues, 0);
        assert_eq!(v.confidence, ConfidenceLevel::High);
    }

    #[test]
    fn test_verification_failed() {
        let v = VerificationResult::failed(3, 2, "3 tests failed".into(), "Report".into());
        assert!(!v.passed);
        assert_eq!(v.blocking_issues, 3);
        assert_eq!(v.advisory_issues, 2);
        assert_eq!(v.confidence, ConfidenceLevel::Low);
    }
}
