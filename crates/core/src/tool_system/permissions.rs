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
    /// 返回 Option: Some(true) = 确认, Some(false) = 自动通过, None = 拒绝执行。
    pub fn check(
        tool_name: &str,
        mode: PermissionMode,
    ) -> Option<bool> {
        let risk = Self::classify_risk(tool_name);
        match mode {
            PermissionMode::Default => {
                // 高风险需要确认，低/中直接通过
                Some(risk == RiskLevel::High)
            }
            PermissionMode::Plan => {
                // 只允许低风险(只读)，中/高风险拒绝
                if risk == RiskLevel::Low { Some(false) } else { None }
            }
            PermissionMode::Yolo => {
                // 全放行
                Some(false)
            }
            PermissionMode::Chat => {
                // 全拒绝
                None
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
    fn test_default_mode_asks_for_high_risk() {
        assert_eq!(
            ToolPermissionChecker::check("bash", PermissionMode::Default),
            Some(true) // needs approval
        );
    }

    #[test]
    fn test_default_mode_allows_low_risk() {
        assert_eq!(
            ToolPermissionChecker::check("file_read", PermissionMode::Default),
            Some(false) // auto allow
        );
    }

    #[test]
    fn test_plan_mode_denies_writes() {
        assert_eq!(
            ToolPermissionChecker::check("file_write", PermissionMode::Plan),
            None // denied
        );
    }

    #[test]
    fn test_plan_mode_allows_reads() {
        assert_eq!(
            ToolPermissionChecker::check("file_read", PermissionMode::Plan),
            Some(false) // auto allow
        );
    }

    #[test]
    fn test_yolo_mode_allows_all() {
        assert_eq!(
            ToolPermissionChecker::check("bash", PermissionMode::Yolo),
            Some(false)
        );
    }

    #[test]
    fn test_chat_mode_denies_all() {
        assert_eq!(
            ToolPermissionChecker::check("file_read", PermissionMode::Chat),
            None // denied
        );
    }
}
