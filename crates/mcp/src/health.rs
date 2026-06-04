use serde::{Deserialize, Serialize};

/// 系统健康状态快照。
/// 各字段由对应的子系统在启动后定期更新。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// LLM API 是否可达 (ping DeepSeek API)
    pub llm_reachable: bool,
    /// SQLite 是否可读写 (PRAGMA integrity_check)
    pub sqlite_readable: bool,
    /// 记忆存储是否可用 (MemoryStore::node_count() 不报错)
    pub memory_operational: bool,
    /// 沙箱后端是否可用 (SandboxBackend::is_available())
    pub sandbox_available: bool,
    /// 代码图谱存储是否可用 (GraphStore::node_count() 不报错)
    pub graph_store_ok: bool,
}

impl HealthStatus {
    /// 创建默认不健康状态。子系统初始化后各自更新对应字段。
    pub fn new() -> Self {
        Self {
            llm_reachable: false,
            sqlite_readable: false,
            memory_operational: false,
            sandbox_available: false,
            graph_store_ok: false,
        }
    }

    /// 所有子系统是否就绪。
    pub fn all_healthy(&self) -> bool {
        self.llm_reachable
            && self.sqlite_readable
            && self.memory_operational
            && self.sandbox_available
            && self.graph_store_ok
    }

    /// 返回不健康的子系统列表。
    pub fn unhealthy_components(&self) -> Vec<&'static str> {
        let mut components = Vec::new();
        if !self.llm_reachable {
            components.push("llm");
        }
        if !self.sqlite_readable {
            components.push("sqlite");
        }
        if !self.memory_operational {
            components.push("memory");
        }
        if !self.sandbox_available {
            components.push("sandbox");
        }
        if !self.graph_store_ok {
            components.push("code_graph");
        }
        components
    }
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_all_unhealthy() {
        let status = HealthStatus::new();
        assert!(!status.all_healthy());
        assert_eq!(status.unhealthy_components().len(), 5);
    }

    #[test]
    fn test_all_healthy() {
        let status = HealthStatus {
            llm_reachable: true,
            sqlite_readable: true,
            memory_operational: true,
            sandbox_available: true,
            graph_store_ok: true,
        };
        assert!(status.all_healthy());
        assert!(status.unhealthy_components().is_empty());
    }

    #[test]
    fn test_partial_unhealthy() {
        let status = HealthStatus {
            llm_reachable: true,
            sqlite_readable: true,
            memory_operational: false,
            sandbox_available: true,
            graph_store_ok: false,
        };
        assert!(!status.all_healthy());
        let unhealthy = status.unhealthy_components();
        assert_eq!(unhealthy.len(), 2);
        assert!(unhealthy.contains(&"memory"));
        assert!(unhealthy.contains(&"code_graph"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let status = HealthStatus {
            llm_reachable: true,
            sqlite_readable: true,
            memory_operational: true,
            sandbox_available: false,
            graph_store_ok: true,
        };
        let json = serde_json::to_string(&status).unwrap();
        let decoded: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.llm_reachable, status.llm_reachable);
        assert_eq!(decoded.sandbox_available, status.sandbox_available);
    }
}
