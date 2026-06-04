use std::time::Duration;

/// 沙箱实例的执行结果。
#[derive(Debug, Clone)]
pub struct SandboxResult {
    /// 退出码
    pub exit_code: i32,
    /// 标准输出
    pub stdout: String,
    /// 标准错误
    pub stderr: String,
    /// 执行耗时
    pub elapsed: Duration,
    /// 是否被信号杀死 (OOM, 超时等)
    pub killed: bool,
}

/// 沙箱权限配置。控制沙箱内进程可访问的资源。
#[derive(Debug, Clone, Default)]
pub struct SandboxPermissions {
    /// 可读的文件/目录路径列表 (glob 模式)
    pub read_paths: Vec<String>,
    /// 可写的文件/目录路径列表
    pub write_paths: Vec<String>,
    /// 是否允许网络访问
    pub network_enabled: bool,
    /// 允许访问的域名 (仅 network_enabled=true 时生效)
    pub allowed_domains: Vec<String>,
    /// 最大执行时间
    pub max_execution_time: Duration,
    /// 最大内存 (MB), 0=不限制
    pub max_memory_mb: u64,
    /// 最大文件大小 (MB)
    pub max_file_size_mb: u64,
}

impl SandboxPermissions {
    /// 只读工作区权限 (最常用)。
    pub fn read_only_workspace(workspace: &str) -> Self {
        Self {
            read_paths: vec![workspace.to_string()],
            write_paths: vec![],
            network_enabled: false,
            allowed_domains: vec![],
            max_execution_time: Duration::from_secs(120),
            max_memory_mb: 512,
            max_file_size_mb: 256,
        }
    }

    /// 读写工作区权限。
    pub fn read_write_workspace(workspace: &str) -> Self {
        Self {
            read_paths: vec![workspace.to_string()],
            write_paths: vec![workspace.to_string()],
            network_enabled: false,
            allowed_domains: vec![],
            max_execution_time: Duration::from_secs(300),
            max_memory_mb: 1024,
            max_file_size_mb: 512,
        }
    }

    /// 完全访问权限 (仅 YOLO 模式)。
    pub fn full_access() -> Self {
        Self {
            read_paths: vec!["/".to_string()],
            write_paths: vec!["/".to_string()],
            network_enabled: true,
            allowed_domains: vec!["*".to_string()],
            max_execution_time: Duration::from_secs(600),
            max_memory_mb: 4096,
            max_file_size_mb: 1024,
        }
    }
}

/// 沙箱后端抽象。所有沙箱实现 (Landlock, Firecracker, Zeroboot) 必须实现此 trait。
/// trait 定义在 core 中，实现在 sandbox crate 中。
pub trait SandboxBackend: Send + Sync {
    /// 创建新的沙箱实例。
    fn spawn(&self, permissions: SandboxPermissions) -> Result<Box<dyn SandboxInstance>, crate::error::AgentError>;

    /// 检查此后端在当前平台上是否可用。
    /// e.g., Landlock 仅 Linux 5.13+, Firecracker 仅 Linux + KVM。
    fn is_available() -> bool
    where
        Self: Sized;
}

/// 沙箱实例。代表一个隔离的执行环境。
pub trait SandboxInstance: Send {
    /// 在沙箱内执行命令。
    fn execute(&mut self, command: &str, args: &[&str]) -> Result<SandboxResult, crate::error::AgentError>;

    /// 向沙箱写入文件。
    fn write_file(&mut self, path: &str, content: &str) -> Result<(), crate::error::AgentError>;

    /// 从沙箱读取文件。
    fn read_file(&self, path: &str) -> Result<String, crate::error::AgentError>;

    /// 创建沙箱快照 (仅 Firecracker/Zeroboot 支持, Landlock 返回 Ok)。
    fn checkpoint(&self, name: &str) -> Result<(), crate::error::AgentError>;

    /// 回滚到指定快照。
    fn restore(&mut self, name: &str) -> Result<(), crate::error::AgentError>;

    /// 沙箱是否仍存活。
    fn is_alive(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_permissions_no_network() {
        let perms = SandboxPermissions::read_only_workspace("/tmp/test");
        assert!(!perms.network_enabled);
        assert!(perms.write_paths.is_empty());
        assert!(!perms.read_paths.is_empty());
    }

    #[test]
    fn test_read_write_permissions() {
        let perms = SandboxPermissions::read_write_workspace("/tmp/test");
        assert!(!perms.network_enabled);
        assert_eq!(perms.read_paths, perms.write_paths);
    }

    #[test]
    fn test_full_access_permissions() {
        let perms = SandboxPermissions::full_access();
        assert!(perms.network_enabled);
        assert_eq!(perms.allowed_domains, vec!["*"]);
    }

    #[test]
    fn test_sandbox_result_defaults() {
        let result = SandboxResult {
            exit_code: 0,
            stdout: "ok".into(),
            stderr: String::new(),
            elapsed: Duration::from_secs(1),
            killed: false,
        };
        assert_eq!(result.exit_code, 0);
        assert!(!result.killed);
    }
}
