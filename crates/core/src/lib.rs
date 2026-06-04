// aegis-core: 核心基础设施 (耦合最强, 最先实现)
// 包含类型系统、错误类型、LLM客户端、工具系统、Agent循环、上下文管理等

pub mod agent;
pub mod constants;
pub mod error;
pub mod hooks;
pub mod job;
pub mod llm;
pub mod lsp;
pub mod migrations;
pub mod network;
pub mod permissions;
pub mod shutdown;
pub mod skills;
pub mod snapshots;
pub mod tool_system;
pub mod types;
pub mod utils;

pub use error::{AgentError, AgentResult};
