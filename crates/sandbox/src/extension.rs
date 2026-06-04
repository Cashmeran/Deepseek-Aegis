//! Extension traits for SandboxBackend/SandboxInstance (defined in core/types/sandbox.rs).
//! These add management functions not in the core trait to avoid forcing all backends to implement them.

use aegis_core::error::AgentResult;
use aegis_core::types::sandbox::{SandboxBackend, SandboxInstance};
use std::path::Path;

/// Extension trait for backend management (name, isolation level, availability check)
pub trait SandboxBackendExt: SandboxBackend {
    fn name(&self) -> &'static str;
    fn isolation_level(&self) -> IsolationLevel;
}

/// Extension trait for sandbox instance management
pub trait SandboxInstanceExt: SandboxInstance {
    fn kill(&mut self) -> AgentResult<()>;
    fn workspace_root(&self) -> &Path;
}

/// Isolation levels in increasing order of security
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IsolationLevel {
    None = 0,
    Process = 1,
    Namespace = 2,
    Landlock = 3,
    Seccomp = 4,
    FullVM = 5,
}

/// Resource usage snapshot
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    pub cpu_time_ms: u64,
    pub peak_memory_kb: u64,
}
