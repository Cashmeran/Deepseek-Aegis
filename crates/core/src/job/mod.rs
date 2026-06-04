use std::time::Instant;

/// Job 持久化 trait。JobManager 通过此接口将队列写入存储。
pub trait JobStore: Send + Sync {
    fn persist(&self, jobs: &[Job]) -> Result<(), String>;
    fn load(&self) -> Result<Vec<Job>, String>;
}

/// Job 状态。对齐 DS-TUI JobManager 状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// 单个 Job。
#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub description: String,
    pub status: JobStatus,
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub finished_at: Option<Instant>,
    pub error_message: Option<String>,
}

impl Job {
    pub fn new(id: String, description: String) -> Self {
        Self {
            id,
            description,
            status: JobStatus::Queued,
            created_at: Instant::now(),
            started_at: None,
            finished_at: None,
            error_message: None,
        }
    }
}

/// Job 管理器。维护最近 64 个 Job 的历史记录。
pub struct JobManager {
    jobs: Vec<Job>,
    max_history: usize,
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            jobs: Vec::with_capacity(64),
            max_history: 64,
        }
    }

    /// 创建新 Job。
    pub fn create(&mut self, id: &str, description: &str) -> &Job {
        let job = Job::new(id.to_string(), description.to_string());
        self.jobs.push(job);
        self.prune();
        self.jobs.last().expect("job was just pushed — must exist")
    }

    /// 标记 Job 为 Running。
    pub fn start(&mut self, id: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.status = JobStatus::Running;
            job.started_at = Some(Instant::now());
        }
    }

    /// 标记 Job 为 Completed。
    pub fn complete(&mut self, id: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.status = JobStatus::Completed;
            job.finished_at = Some(Instant::now());
        }
    }

    /// 标记 Job 为 Failed。
    pub fn fail(&mut self, id: &str, error: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.status = JobStatus::Failed;
            job.finished_at = Some(Instant::now());
            job.error_message = Some(error.to_string());
        }
    }

    /// 标记 Job 为 Cancelled。
    pub fn cancel(&mut self, id: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.status = JobStatus::Cancelled;
            job.finished_at = Some(Instant::now());
        }
    }

    /// 获取所有 Job (最近 64 个)。
    pub fn all(&self) -> &[Job] {
        &self.jobs
    }

    /// 获取活跃的 Job (非 Completed/Failed/Cancelled)。
    pub fn active(&self) -> Vec<&Job> {
        self.jobs
            .iter()
            .filter(|j| {
                matches!(j.status, JobStatus::Queued | JobStatus::Running)
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    fn prune(&mut self) {
        while self.jobs.len() > self.max_history {
            self.jobs.remove(0);
        }
    }
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_lifecycle() {
        let mut mgr = JobManager::new();
        mgr.create("job-1", "Test job");
        assert_eq!(mgr.len(), 1);

        mgr.start("job-1");
        assert_eq!(mgr.active().len(), 1);

        mgr.complete("job-1");
        assert_eq!(mgr.active().len(), 0);
    }

    #[test]
    fn test_job_failure() {
        let mut mgr = JobManager::new();
        mgr.create("job-2", "Failing job");
        mgr.start("job-2");
        mgr.fail("job-2", "Something went wrong");

        let job = &mgr.all()[0];
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error_message.as_deref(), Some("Something went wrong"));
    }

    #[test]
    fn test_prune_old_jobs() {
        let mut mgr = JobManager::new();
        for i in 0..70 {
            mgr.create(&format!("job-{}", i), "Test");
        }
        assert_eq!(mgr.len(), 64); // 保留最近 64 个
    }
}
