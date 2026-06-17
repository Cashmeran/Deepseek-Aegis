//! SandboxPool — pre-warmed pool with acquire/return model.
//! Uses Arc<Self> to avoid lifetime issues with async.

use aegis_core::error::{AgentError, AgentResult};
use aegis_core::types::sandbox::{SandboxBackend, SandboxInstance, SandboxPermissions, SandboxResult};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub min_idle: usize,
    pub max_total: usize,
    pub max_executions: u32,
    pub idle_timeout_secs: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_idle: 2,
            max_total: 20,
            max_executions: 100,
            idle_timeout_secs: 300,
        }
    }
}

pub struct SandboxPool {
    backend: Box<dyn SandboxBackend>,
    idle: Mutex<VecDeque<Box<dyn SandboxInstance>>>,
    semaphore: Arc<tokio::sync::Semaphore>,
    config: PoolConfig,
}

impl SandboxPool {
    pub fn new(backend: Box<dyn SandboxBackend>, config: PoolConfig) -> AgentResult<Arc<Self>> {
        let pool = Arc::new(Self {
            backend,
            idle: Mutex::new(VecDeque::new()),
            semaphore: Arc::new(tokio::sync::Semaphore::new(config.max_total)),
            config,
        });
        pool.warm_up()?;
        Ok(pool)
    }

    fn warm_up(&self) -> AgentResult<()> {
        let mut idle = self.idle.lock().unwrap();
        while idle.len() < self.config.min_idle {
            let inst = self.backend.spawn(SandboxPermissions::read_only_workspace("."))?;
            idle.push_back(inst);
        }
        Ok(())
    }

    pub async fn acquire(self: &Arc<Self>, perms: SandboxPermissions) -> AgentResult<SandboxGuard> {
        let permit = Arc::clone(&self.semaphore).acquire_owned().await
            .map_err(|_| AgentError::SandboxUnavailable("pool exhausted".to_string()))?;

        let instance = {
            let mut idle = self.idle.lock().unwrap();
            idle.pop_front()
        };

        let instance = match instance {
            Some(inst) => inst,
            None => self.backend.spawn(perms)?,
        };

        Ok(SandboxGuard {
            instance: Some(instance),
            pool: Arc::clone(self),
            _permit: permit,
            executions: 0,
        })
    }

    fn return_instance(&self, instance: Box<dyn SandboxInstance>) {
        let mut idle = self.idle.lock().unwrap();
        if idle.len() < self.config.max_total {
            idle.push_back(instance);
        }
    }
}

pub struct SandboxGuard {
    instance: Option<Box<dyn SandboxInstance>>,
    pool: Arc<SandboxPool>,
    _permit: tokio::sync::OwnedSemaphorePermit,
    executions: u32,
}

impl SandboxGuard {
    pub fn execute(&mut self, cmd: &str, args: &[&str]) -> AgentResult<SandboxResult> {
        self.executions += 1;
        self.instance.as_mut().unwrap().execute(cmd, args)
    }

    pub fn write_file(&mut self, path: &str, content: &str) -> AgentResult<()> {
        self.instance.as_mut().unwrap().write_file(path, content)
    }

    pub fn read_file(&self, path: &str) -> AgentResult<String> {
        self.instance.as_ref().unwrap().read_file(path)
    }

    pub fn is_alive(&self) -> bool {
        self.instance.as_ref().map(|i| i.is_alive()).unwrap_or(false)
    }
}

impl Drop for SandboxGuard {
    fn drop(&mut self) {
        if let Some(inst) = self.instance.take() {
            if inst.is_alive() && self.executions < self.pool.config.max_executions {
                self.pool.return_instance(inst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessBackend;

    #[test]
    fn test_pool_acquire_and_execute() {
        let backend = Box::new(ProcessBackend);
        let config = PoolConfig { min_idle: 1, max_total: 4, ..Default::default() };
        let pool = SandboxPool::new(backend, config).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut guard = rt.block_on(pool.acquire(SandboxPermissions::read_only_workspace("."))).unwrap();
        let (cmd, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd", &["/c", "echo", "test"] as &[&str])
        } else {
            ("echo", &["test"] as &[&str])
        };
        let result = guard.execute(cmd, args).unwrap();
        assert_eq!(result.exit_code, 0);
    }
}
