use crate::error::{AgentError, AgentResult};
use crate::tool_system::registry::ToolRegistry;
use crate::types::message::{ContentBlock, ToolResultMessage, ToolUse};
use crate::types::tool::ToolContext;
use std::sync::Arc;

/// 工具调度器。负责并行安全地批量执行工具调用。
/// 参考 SWE-agent ACI 设计: 简洁有界输出 / 跨轮次持久 / 按调用安全 / 可预测参数
pub struct ToolDispatch {
    registry: Arc<ToolRegistry>,
}

impl ToolDispatch {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }

    /// 分批并行执行工具调用。每批最多 max_parallel 个，超过的分批串行等待。
    /// 单个工具失败不中止整个批次——错误包装在 ToolResultMessage 中继续。
    pub async fn execute_batch(
        &self,
        tool_uses: &[ToolUse],
        ctx: &ToolContext,
        max_parallel: usize,
    ) -> AgentResult<Vec<ToolResultMessage>> {
        if tool_uses.is_empty() {
            return Ok(vec![]);
        }

        let max_parallel = max_parallel.max(1);
        let mut results = Vec::with_capacity(tool_uses.len());

        for chunk in tool_uses.chunks(max_parallel) {
            let mut handles = Vec::with_capacity(chunk.len());

            for tu in chunk {
                let registry = Arc::clone(&self.registry);
                let tu = tu.clone();
                let ctx = ctx.clone();

                let handle = tokio::spawn(async move {
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(ctx.timeout_ms),
                        registry.execute(&tu, &ctx),
                    )
                    .await
                    {
                        Ok(Ok(result)) => result,
                        Ok(Err(e)) => ToolResultMessage {
                            tool_use_id: tu.id.clone(),
                            is_error: true,
                            content: vec![ContentBlock::Text {
                                text: format!("Tool execution error: {}", e),
                            }],
                            elapsed_ms: 0,
                        },
                        Err(_) => ToolResultMessage {
                            tool_use_id: tu.id.clone(),
                            is_error: true,
                            content: vec![ContentBlock::Text {
                                text: format!("Tool timed out after {}ms", ctx.timeout_ms),
                            }],
                            elapsed_ms: ctx.timeout_ms,
                        },
                    }
                });

                handles.push(handle);
            }

            // 等待当前批次全部完成
            for handle in handles {
                match handle.await {
                    Ok(result) => results.push(result),
                    Err(join_err) => {
                        return Err(AgentError::Internal(format!(
                            "Tool task panicked: {}",
                            join_err
                        )));
                    }
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_system::ToolRegistry;
    use crate::types::tool::PermissionMode;

    // 注意: execute_batch 需要 ToolRegistry 中有已注册的工具才能执行。
    // 这些测试验证调度逻辑，不依赖实际工具实现。

    #[tokio::test]
    async fn test_empty_batch() {
        let registry = Arc::new(ToolRegistry::new());
        let dispatch = ToolDispatch::new(registry);
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None,
        };

        let results = dispatch.execute_batch(&[], &ctx, 8).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_execute_nonexistent_tool_returns_error() {
        let registry = Arc::new(ToolRegistry::new());
        let dispatch = ToolDispatch::new(registry);
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None,
        };
        let tool_uses = vec![ToolUse {
            id: "toolu_001".into(),
            name: "ghost_tool".into(),
            input: serde_json::json!({}),
        }];

        let results = dispatch.execute_batch(&tool_uses, &ctx, 8).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_error);
        assert!(results[0]
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { text } if text.contains("Tool"))));
    }
}
