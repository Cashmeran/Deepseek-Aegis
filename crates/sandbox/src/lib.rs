// aegis-sandbox: Lightweight security sandbox
// ProcessBackend (cross-platform) + SandboxPool + Cleanup

pub mod extension;
pub mod process;
pub mod pool;
pub mod cleanup;
pub mod monitor;

pub use extension::{IsolationLevel, ResourceUsage, SandboxBackendExt, SandboxInstanceExt};
pub use process::ProcessBackend;
pub use pool::{PoolConfig, SandboxGuard, SandboxPool};
pub use cleanup::{CleanupReport, SandboxCleanup};
pub use monitor::SandboxMonitor;
