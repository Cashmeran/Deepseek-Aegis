use crate::types::tool::{PermissionMode, RiskLevel};

/// 权限检查结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionResult {
    /// 自动放行
    Allow,
    /// 需要用户确认
    Ask,
    /// 拒绝执行
    Deny,
}

/// 权限检查器 trait。所有 crate 间权限判断通过此接口。
/// 实现: core::permissions (默认规则) 或外部插件（未来）。
pub trait PermissionChecker: Send + Sync {
    /// 检查工具调用是否需要用户确认。
    fn check(&self, tool_name: &str, risk: RiskLevel, mode: PermissionMode) -> PermissionResult;

    /// 分类工具的风险等级。
    fn classify_risk(&self, tool_name: &str) -> RiskLevel;
}

/// 默认权限检查器实现。
pub struct DefaultPermissionChecker;

impl PermissionChecker for DefaultPermissionChecker {
    fn check(&self, _tool_name: &str, risk: RiskLevel, mode: PermissionMode) -> PermissionResult {
        match mode {
            PermissionMode::Default => match risk {
                RiskLevel::High => PermissionResult::Ask,
                _ => PermissionResult::Allow,
            },
            PermissionMode::Auto => match risk {
                RiskLevel::High => PermissionResult::Ask,
                _ => PermissionResult::Allow,
            },
            PermissionMode::Bypass | PermissionMode::Yolo => PermissionResult::Allow,
        }
    }

    fn classify_risk(&self, tool_name: &str) -> RiskLevel {
        match tool_name {
            "bash" | "file_write" | "file_edit" => RiskLevel::High,
            "web_fetch" => RiskLevel::Medium,
            "file_read" | "glob" | "grep" => RiskLevel::Low,
            _ => RiskLevel::High, // 未知工具默认高风险
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_high_risk_asks() {
        let checker = DefaultPermissionChecker;
        assert_eq!(
            checker.check("bash", RiskLevel::High, PermissionMode::Default),
            PermissionResult::Ask
        );
    }

    #[test]
    fn test_bypass_always_allows() {
        let checker = DefaultPermissionChecker;
        assert_eq!(
            checker.check("bash", RiskLevel::High, PermissionMode::Bypass),
            PermissionResult::Allow
        );
    }

    #[test]
    fn test_classify_risk() {
        let checker = DefaultPermissionChecker;
        assert_eq!(checker.classify_risk("bash"), RiskLevel::High);
        assert_eq!(checker.classify_risk("file_read"), RiskLevel::Low);
    }
}
