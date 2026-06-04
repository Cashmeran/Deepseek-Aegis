use crate::types::tool::{RiskLevel, PermissionMode};

/// 工具权限检查器。根据风险等级和权限模式决定是否放行。
pub struct ToolPermissionChecker;

impl ToolPermissionChecker {
    /// 分类工具的风险等级。
    /// 基于工具的 schema 名称和已知风险模式。
    pub fn classify_risk(tool_name: &str) -> RiskLevel {
        match tool_name {
            // 高危险: 写入 + 执行
            "bash" | "file_write" | "file_edit" => RiskLevel::High,
            // 中风险: 网络操作
            "web_fetch" => RiskLevel::Medium,
            // 低风险: 只读
            "file_read" | "glob" | "grep" => RiskLevel::Low,
            // 未知工具: 默认高风险 (最小权限原则)
            _ => RiskLevel::High,
        }
    }

    /// 检查给定操作是否需要用户确认。
    /// 返回 true = 需要确认，false = 自动通过。
    pub fn requires_approval(
        risk: RiskLevel,
        mode: PermissionMode,
    ) -> bool {
        match mode {
            PermissionMode::Default => {
                // 默认: 高风险需要确认
                matches!(risk, RiskLevel::High)
            }
            PermissionMode::Auto => {
                // 自动: 只有高风险需要确认 (ML 置信度由 AgentLoop 控制)
                matches!(risk, RiskLevel::High)
            }
            PermissionMode::Bypass | PermissionMode::Yolo => {
                // 全放行: 不需要确认
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_bash_high_risk() {
        assert_eq!(
            ToolPermissionChecker::classify_risk("bash"),
            RiskLevel::High
        );
    }

    #[test]
    fn test_classify_file_read_low_risk() {
        assert_eq!(
            ToolPermissionChecker::classify_risk("file_read"),
            RiskLevel::Low
        );
    }

    #[test]
    fn test_classify_unknown_high_risk() {
        assert_eq!(
            ToolPermissionChecker::classify_risk("unknown_tool"),
            RiskLevel::High
        );
    }

    #[test]
    fn test_default_mode_requires_approval_for_high_risk() {
        assert!(ToolPermissionChecker::requires_approval(
            RiskLevel::High,
            PermissionMode::Default
        ));
    }

    #[test]
    fn test_default_mode_auto_passes_low_risk() {
        assert!(!ToolPermissionChecker::requires_approval(
            RiskLevel::Low,
            PermissionMode::Default
        ));
    }

    #[test]
    fn test_bypass_mode_never_requires_approval() {
        assert!(!ToolPermissionChecker::requires_approval(
            RiskLevel::High,
            PermissionMode::Bypass
        ));
    }
}
