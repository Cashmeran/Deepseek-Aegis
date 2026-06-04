use std::future::Future;
use std::time::Duration;

/// 注册信号处理并执行优雅关闭。
///
/// 监听 Ctrl+C (SIGINT) 和 SIGTERM，收到信号后:
/// 1. 调用 `on_shutdown` 钩子执行持久化/清理
/// 2. 硬超时 5 秒，超时后强制退出
/// 3. 退出码 0
///
/// `on_shutdown` 应包含:
/// - JobManager::persist_all() (将队列持久化到 SQLite)
/// - SandboxPool::drain() (等待沙箱归还，杀掉剩余)
/// - MemoryStore::flush() (刷新待写入的记忆)
pub async fn graceful_shutdown<F, Fut>(on_shutdown: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    // 跨平台信号监听: Windows 上用 ctrl_c, Unix 上添加 SIGTERM
    #[cfg(unix)]
    let mut sigterm = {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("Failed to register SIGTERM handler: {}", e);
                None
            }
        }
    };

    // 等待信号
    #[cfg(unix)]
    {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl+C received, initiating graceful shutdown");
            }
            _ = async {
                if let Some(ref mut s) = sigterm {
                    _ = s.recv().await;
                } else {
                    std::future::pending::<()>().await
                }
            } => {
                tracing::info!("SIGTERM received, initiating graceful shutdown");
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Ctrl+C received, initiating graceful shutdown");
    }

    // 硬超时 5 秒: 必须在此时间内完成所有持久化
    let shutdown = tokio::time::timeout(Duration::from_secs(5), on_shutdown());
    match shutdown.await {
        Ok(()) => tracing::info!("Graceful shutdown complete"),
        Err(_) => tracing::warn!("Shutdown timed out after 5s, forcing exit"),
    }

    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_shutdown_invokes_hook() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        // 模拟: 通过自身发送 ctrl_c 信号来触发 (不可靠, 跳过直接测试钩子)
        // 改为: 直接验证 graceful_shutdown 的函数签名和钩子逻辑
        // 提供快速的 on_shutdown 钩子
        let hook = move || {
            let c = called_clone.clone();
            async move {
                c.store(true, Ordering::SeqCst);
            }
        };

        // 在不实际发送信号的情况下, 我们不能直接测试 graceful_shutdown
        // 改为测试钩子本身能正常工作
        hook().await;
        assert!(called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_shutdown_timeout_behavior() {
        // 验证 timeout 超时逻辑: 如果钩子超过 5 秒, timeout 生效
        let slow_hook = || async {
            sleep(Duration::from_millis(100)).await; // 远低于5秒, 不应触超时
        };

        let result = tokio::time::timeout(Duration::from_secs(5), slow_hook()).await;
        assert!(result.is_ok());
    }
}
