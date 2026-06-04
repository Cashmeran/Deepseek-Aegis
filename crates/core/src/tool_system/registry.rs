use crate::error::{AgentError, AgentResult};
use crate::types::message::{ToolResultMessage, ToolUse};
use crate::types::tool::{
    ConcurrencySafety, ExecutionMode, RiskLevel, Tool, ToolContext, ToolSchema,
};
use dashmap::DashMap;
use std::sync::{Arc, RwLock};

/// 单条工具注册项。包含工具实例、缓存schema、并发和风险级别。
struct ToolEntry {
    tool: Arc<dyn Tool>,
    schema: ToolSchema,
    concurrency: ConcurrencySafety,
    risk: RiskLevel,
}

/// 工具注册中心。线程安全，支持并发读。
/// DashMap::with_capacity(32): 15个标准工具 + 10个MCP工具 + 7个预留
pub struct ToolRegistry {
    tools: DashMap<String, ToolEntry>,
    /// JSON Schema缓存: [{"name":"bash","description":"...","input_schema":{...}}, ...]
    /// 系统提示构建时直接返回缓存的JSON, 避免每次重新序列化
    /// 使用 std::sync::RwLock —— register/unregister 是同步方法,
    /// 不涉及 tokio 异步上下文
    cached_tool_list_json: RwLock<String>,
}

impl ToolRegistry {
    /// 创建空的工具注册中心。预分配32个槽位。
    pub fn new() -> Self {
        Self {
            tools: DashMap::with_capacity(32),
            cached_tool_list_json: RwLock::new("[]".to_string()),
        }
    }

    /// 注册一个工具实例。如果同名工具已存在, 返回错误。
    /// 注册后自动重建JSON Schema缓存。
    /// 使用 insert 返回值做原子重复检查，避免 TOCTOU 竞争。
    pub fn register(&self, tool: Arc<dyn Tool>) -> AgentResult<()> {
        let schema = tool.schema();
        let name = schema.name.clone();
        let concurrency = tool.concurrency_safety();
        let risk = tool.risk_level();

        let entry = ToolEntry {
            tool,
            schema,
            concurrency,
            risk,
        };

        let old = self.tools.insert(name.clone(), entry);
        if old.is_some() {
            return Err(AgentError::ToolValidationError {
                tool: name.clone(),
                errors: format!("Tool '{}' is already registered", name),
            });
        }

        self.rebuild_cache();
        Ok(())
    }

    /// 执行工具调用。查找工具→验证参数→执行。
    pub async fn execute(
        &self,
        tool_use: &ToolUse,
        ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage> {
        let entry = self
            .tools
            .get(&tool_use.name)
            .ok_or_else(|| AgentError::ToolNotFound {
                name: tool_use.name.clone(),
                available: self.tool_names().join(", "),
            })?;

        // 验证输入参数
        entry.tool.validate_input(&tool_use.input)?;

        // 执行工具 (Arc clone = 引用计数增加, 不复制工具实例)
        let tool = Arc::clone(&entry.tool);
        tool.execute(tool_use, ctx).await
    }

    /// 返回所有已注册工具的Schema列表。
    pub fn list_schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .iter()
            .map(|entry| entry.schema.clone())
            .collect()
    }

    /// 按执行模式过滤工具Schema。
    /// Plan模式: 仅返回只读工具 (RiskLevel::Low)
    /// Agent/Yolo模式: 返回全部工具
    /// Chat模式: 返回空
    pub fn list_schemas_for_mode(&self, mode: ExecutionMode) -> Vec<ToolSchema> {
        match mode {
            ExecutionMode::Chat => vec![],
            ExecutionMode::Plan => self
                .tools
                .iter()
                .filter(|entry| entry.risk == RiskLevel::Low)
                .map(|entry| entry.schema.clone())
                .collect(),
            ExecutionMode::Default | ExecutionMode::Yolo => self.list_schemas(),
        }
    }

    /// 查询单个工具的Schema。
    pub fn get_schema(&self, name: &str) -> Option<ToolSchema> {
        self.tools.get(name).map(|entry| entry.schema.clone())
    }

    /// 查询工具的并发安全性。
    pub fn get_concurrency(&self, name: &str) -> Option<ConcurrencySafety> {
        self.tools.get(name).map(|entry| entry.concurrency)
    }

    /// 查询工具的风险等级。
    pub fn get_risk(&self, name: &str) -> Option<RiskLevel> {
        self.tools.get(name).map(|entry| entry.risk)
    }

    /// 已注册工具数量。
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// 查询工具的并发安全性。不存在返回 ConcurrentSafe（默认）。
    pub fn concurrency_safety_of(&self, name: &str) -> crate::types::tool::ConcurrencySafety {
        self.tools.get(name)
            .map(|e| e.concurrency)
            .unwrap_or(crate::types::tool::ConcurrencySafety::ConcurrentSafe)
    }

    /// 检查工具是否已注册。
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// 获取缓存的工具列表JSON。系统提示构建时使用。
    pub fn get_cached_tool_list_json(&self) -> String {
        self.cached_tool_list_json
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| "[]".to_string())
    }

    /// 获取 DeepSeek API 格式的工具定义 (用于 API 请求的 tools 字段)。
    pub fn get_anthropic_tools_json(&self) -> String {
        let tools: Vec<serde_json::Value> = self.tools.iter().map(|entry| {
            let schema = &entry.value().schema;
            serde_json::json!({
                "name": schema.name,
                "description": schema.description,
                "input_schema": schema.input_schema,
            })
        }).collect();
        serde_json::to_string(&tools).unwrap_or_else(|_| "[]".to_string())
    }

    /// 注销一个工具。成功返回true, 不存在返回false。
    pub fn unregister(&self, name: &str) -> bool {
        let removed = self.tools.remove(name).is_some();
        if removed {
            self.rebuild_cache();
        }
        removed
    }

    // ── 内部方法 ──

    /// 返回所有已注册工具名。
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Clone a tool by name (Arc clone, not deep copy). For sub-agent registry construction.
    pub fn get_clone(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).map(|entry| Arc::clone(&entry.tool))
    }

    /// 重建缓存的 JSON Schema 列表。
    fn rebuild_cache(&self) {
        let schemas: Vec<serde_json::Value> = self
            .tools
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "name": entry.schema.name,
                    "description": entry.schema.description,
                    "input_schema": entry.schema.input_schema,
                })
            })
            .collect();

        let json = serde_json::to_string(&schemas).unwrap_or_else(|_| "[]".to_string());

        // 写入缓存。register/unregister 是同步方法。
        // 锁中毒 (PoisonError) 时强行覆盖: 缓存正确性优先于中毒保护，
        // 前一持锁者的 panic 不应该让后续所有查询返回过期数据。
        let mut cache = self.cached_tool_list_json.write().unwrap_or_else(|e| {
            tracing::warn!("ToolRegistry cache lock poisoned, recovering: {}", e);
            e.into_inner()
        });
        *cache = json;
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::ContentBlock;
    use crate::types::tool::ToolMetadata;
    use async_trait::async_trait;

    /// 测试用的模拟工具
    struct MockTool {
        name: String,
        risk: RiskLevel,
        concurrency: ConcurrencySafety,
    }

    impl ToolMetadata for MockTool {
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: self.name.clone(),
                description: format!("Mock tool: {}", self.name),
                prompt: String::new(),
                input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
            }
        }

        fn risk_level(&self) -> RiskLevel {
            self.risk
        }

        fn concurrency_safety(&self) -> ConcurrencySafety {
            self.concurrency
        }
    }

    #[async_trait]
    impl Tool for MockTool {
        async fn execute(
            self: Arc<Self>,
            _tool_use: &ToolUse,
            _ctx: &ToolContext,
        ) -> AgentResult<ToolResultMessage> {
            Ok(ToolResultMessage {
                tool_use_id: _tool_use.id.clone(),
                is_error: false,
                content: vec![ContentBlock::Text {
                    text: "mock result".into(),
                }],
                elapsed_ms: 1,
            })
        }
    }

    fn test_registry() -> ToolRegistry {
        let registry = ToolRegistry::new();
        registry
            .register(Arc::new(MockTool {
                name: "read".into(),
                risk: RiskLevel::Low,
                concurrency: ConcurrencySafety::ConcurrentSafe,
            }))
            .unwrap();
        registry
            .register(Arc::new(MockTool {
                name: "write".into(),
                risk: RiskLevel::High,
                concurrency: ConcurrencySafety::ConcurrentUnsafe,
            }))
            .unwrap();
        registry
    }

    #[test]
    fn test_register_and_lookup() {
        let registry = test_registry();
        assert_eq!(registry.len(), 2);
        assert!(registry.contains("read"));
        assert!(registry.contains("write"));
        assert!(!registry.contains("nonexistent"));
    }

    #[test]
    fn test_register_duplicate_fails() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(MockTool {
            name: "dup".into(),
            risk: RiskLevel::Low,
            concurrency: ConcurrencySafety::ConcurrentSafe,
        });
        assert!(registry.register(tool.clone()).is_ok());
        assert!(registry.register(tool).is_err());
    }

    #[test]
    fn test_get_schema() {
        let registry = test_registry();
        let schema = registry.get_schema("read").unwrap();
        assert_eq!(schema.name, "read");
        assert!(registry.get_schema("ghost").is_none());
    }

    #[test]
    fn test_list_schemas() {
        let registry = test_registry();
        let schemas = registry.list_schemas();
        assert_eq!(schemas.len(), 2);
    }

    #[test]
    fn test_list_schemas_for_mode_plan() {
        let registry = test_registry();
        let schemas = registry.list_schemas_for_mode(ExecutionMode::Plan);
        // Plan模式只应返回Low风险工具
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "read");
    }

    #[test]
    fn test_list_schemas_for_mode_chat() {
        let registry = test_registry();
        let schemas = registry.list_schemas_for_mode(ExecutionMode::Chat);
        assert!(schemas.is_empty());
    }

    #[test]
    fn test_unregister() {
        let registry = test_registry();
        assert!(registry.unregister("read"));
        assert_eq!(registry.len(), 1);
        assert!(!registry.unregister("read")); // 重复删除返回false
    }

    #[test]
    fn test_get_risk_and_concurrency() {
        let registry = test_registry();
        assert_eq!(registry.get_risk("read"), Some(RiskLevel::Low));
        assert_eq!(registry.get_risk("write"), Some(RiskLevel::High));
        assert_eq!(
            registry.get_concurrency("read"),
            Some(ConcurrencySafety::ConcurrentSafe)
        );
        assert_eq!(
            registry.get_concurrency("write"),
            Some(ConcurrencySafety::ConcurrentUnsafe)
        );
    }

    #[tokio::test]
    async fn test_execute_tool() {
        let registry = test_registry();
        let ctx = ToolContext {
            working_dir: std::path::PathBuf::from("."),
            permission_mode: crate::types::tool::PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let tool_use = ToolUse {
            id: "toolu_test".into(),
            name: "read".into(),
            input: serde_json::json!({}),
        };
        let result = registry.execute(&tool_use, &ctx).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.tool_use_id, "toolu_test");
    }

    #[tokio::test]
    async fn test_execute_nonexistent_tool_fails() {
        let registry = test_registry();
        let ctx = ToolContext {
            working_dir: std::path::PathBuf::from("."),
            permission_mode: crate::types::tool::PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let tool_use = ToolUse {
            id: "toolu_test".into(),
            name: "ghost".into(),
            input: serde_json::json!({}),
        };
        let err = registry.execute(&tool_use, &ctx).await.unwrap_err();
        match err {
            AgentError::ToolNotFound { name, .. } => assert_eq!(name, "ghost"),
            _ => panic!("Expected ToolNotFound"),
        }
    }
}
