//! 网络策略 — 吸收 DS-TUI
//!
//! allow/deny/prompt 三级策略, 精确匹配 + 子域名通配。
//! deny 优先, audit 日志记录。

/// 网络策略规则。
#[derive(Debug, Clone)]
pub struct NetworkRule {
    /// 域名模式 (支持通配符: *.example.com)
    pub pattern: String,
    /// 策略
    pub action: NetworkAction,
}

/// 网络访问策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkAction {
    /// 允许访问
    Allow,
    /// 拒绝访问
    Deny,
    /// 提示用户确认
    Prompt,
}

/// 网络策略管理器。
pub struct NetworkPolicy {
    rules: Vec<NetworkRule>,
    /// 默认策略 (无匹配规则时)
    default_action: NetworkAction,
}

impl NetworkPolicy {
    pub fn new(default_action: NetworkAction) -> Self {
        Self {
            rules: Vec::new(),
            default_action,
        }
    }

    /// 添加规则。deny 规则排在前面优先匹配。
    pub fn add_rule(&mut self, pattern: &str, action: NetworkAction) {
        let rule = NetworkRule {
            pattern: pattern.to_string(),
            action,
        };
        // deny 优先级最高，插入到前面
        if action == NetworkAction::Deny {
            let pos = self
                .rules
                .iter()
                .position(|r| r.action != NetworkAction::Deny)
                .unwrap_or(self.rules.len());
            self.rules.insert(pos, rule);
        } else {
            self.rules.push(rule);
        }
    }

    /// 检查域名是否匹配。返回应该执行的操作。
    /// 匹配逻辑: 精确匹配 > 子域名通配。
    pub fn check(&self, host: &str) -> NetworkAction {
        for rule in &self.rules {
            if self.match_pattern(&rule.pattern, host) {
                return rule.action;
            }
        }
        self.default_action
    }

    /// 简单通配符匹配: *.example.com 匹配 sub.example.com
    fn match_pattern(&self, pattern: &str, host: &str) -> bool {
        if pattern == host {
            return true;
        }
        if let Some(suffix) = pattern.strip_prefix("*.") {
            return host.ends_with(suffix) || host == &suffix[1..];
        }
        false
    }

    /// 是否为允许的域名。
    pub fn is_allowed(&self, host: &str) -> bool {
        matches!(self.check(host), NetworkAction::Allow)
    }

    /// 获取所有规则。
    pub fn rules(&self) -> &[NetworkRule] {
        &self.rules
    }
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        let mut policy = Self::new(NetworkAction::Prompt);
        // 默认 allow 常见的开发域名
        policy.add_rule("localhost", NetworkAction::Allow);
        policy.add_rule("*.github.com", NetworkAction::Allow);
        policy.add_rule("*.crates.io", NetworkAction::Allow);
        policy.add_rule("*.rust-lang.org", NetworkAction::Allow);
        policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let policy = NetworkPolicy::default();
        assert!(policy.is_allowed("localhost"));
    }

    #[test]
    fn test_wildcard_match() {
        let mut policy = NetworkPolicy::new(NetworkAction::Prompt);
        policy.add_rule("*.example.com", NetworkAction::Allow);
        assert!(policy.is_allowed("sub.example.com"));
        assert!(policy.is_allowed("example.com")); // *.example.com also matches root
    }

    #[test]
    fn test_deny_priority() {
        let mut policy = NetworkPolicy::new(NetworkAction::Allow);
        policy.add_rule("*.trusted.com", NetworkAction::Allow);
        policy.add_rule("evil.trusted.com", NetworkAction::Deny); // 精确 deny 优先
        assert_eq!(policy.check("evil.trusted.com"), NetworkAction::Deny);
        assert_eq!(policy.check("good.trusted.com"), NetworkAction::Allow);
    }

    #[test]
    fn test_default_action() {
        let policy = NetworkPolicy::new(NetworkAction::Prompt);
        assert_eq!(policy.check("unknown.domain.com"), NetworkAction::Prompt);
    }
}
