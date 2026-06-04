//! Three-body harness: Planner → Generator → Evaluator.
//! SprintContract + PlanTask + VerificationSuite.
//! Integrated with plan tool, todo_write, and CodeScorer.

use crate::agent::loop_::TodoStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single acceptance criterion defined by the Planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub description: String,
    pub verification_command: String,
    pub expected_exit_code: i32,
    pub expected_output_contains: Option<String>,
}

/// A plan task — maps directly to a todo_write item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    pub subject: String,
    pub description: String,
    pub depends_on: Vec<String>,
    pub status: TaskState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState { Pending, InProgress, Completed, Blocked }

/// SprintContract — the binding agreement between Planner, Generator, and Evaluator.
/// Created by the plan tool, tracked through todo_write, verified by Evaluator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SprintContract {
    pub objective: String,
    pub task_spec: String,
    pub expected_files: Vec<String>,
    pub tasks: Vec<PlanTask>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub constraints: Vec<String>,
    pub complexity: f32,
    pub estimated_tokens: u64,
    pub plan_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl SprintContract {
    pub fn new(objective: String) -> Self {
        Self {
            task_spec: objective.clone(),
            objective,
            expected_files: vec![],
            tasks: vec![],
            acceptance_criteria: vec![],
            constraints: vec![],
            complexity: 0.0,
            estimated_tokens: 0,
            plan_id: format!("plan_{}", uuid::Uuid::new_v4()),
            created_at: chrono::Utc::now(),
        }
    }

    /// Auto-create from plan tool JSON output.
    pub fn from_plan_json(plan: &serde_json::Value) -> Option<Self> {
        let objective = plan.get("objective")?.as_str()?.to_string();
        let mut contract = Self::new(objective);

        if let Some(files) = plan.get("files").and_then(|v| v.as_array()) {
            contract.expected_files = files.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
        }

        if let Some(tasks) = plan.get("tasks").and_then(|v| v.as_array()) {
            contract.tasks = tasks.iter().map(|t| PlanTask {
                subject: t.get("subject").and_then(|v| v.as_str()).unwrap_or("").into(),
                description: t.get("description").and_then(|v| v.as_str()).unwrap_or("").into(),
                depends_on: t.get("depends_on").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default(),
                status: TaskState::Pending,
            }).collect();
        }

        if let Some(acceptance) = plan.get("acceptance").and_then(|v| v.as_array()) {
            contract.acceptance_criteria = acceptance.iter().map(|a| AcceptanceCriterion {
                description: a.get("description").and_then(|v| v.as_str()).unwrap_or("").into(),
                verification_command: a.get("command").and_then(|v| v.as_str()).unwrap_or("").into(),
                expected_exit_code: a.get("expected_exit").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                expected_output_contains: a.get("expected_contains").and_then(|v| v.as_str()).map(|s| s.to_string()),
            }).collect();
        }

        if let Some(constraints) = plan.get("constraints").and_then(|v| v.as_array()) {
            contract.constraints = constraints.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
        }

        contract.complexity = (contract.tasks.len() as f32 * 0.1).min(1.0);
        contract.estimated_tokens = contract.tasks.len() as u64 * 500;

        Some(contract)
    }

    /// Check plan progress: how many tasks are completed vs total.
    pub fn progress(&self) -> (usize, usize) {
        let completed = self.tasks.iter().filter(|t| t.status == TaskState::Completed).count();
        (completed, self.tasks.len())
    }

    /// Are all plan tasks done?
    pub fn is_complete(&self) -> bool {
        if self.tasks.is_empty() { return true; }
        self.tasks.iter().all(|t| t.status == TaskState::Completed)
    }

    /// Are there any blocked tasks (unmet dependencies)?
    pub fn blocked_tasks(&self) -> Vec<&PlanTask> {
        self.tasks.iter().filter(|t| {
            t.status == TaskState::Pending && t.depends_on.iter().any(|dep| {
                self.tasks.iter().any(|other| other.subject == *dep && other.status != TaskState::Completed)
            })
        }).collect()
    }

    /// Sync task status from todo_write items (called after each todo_write call).
    pub fn sync_todos(&mut self, todos: &[(String, TodoStatus)]) {
        let status_map: HashMap<&str, TaskState> = todos.iter().map(|(subj, status)| {
            (subj.as_str(), match status {
                TodoStatus::Completed => TaskState::Completed,
                TodoStatus::InProgress => TaskState::InProgress,
                TodoStatus::Pending => TaskState::Pending,
            })
        }).collect();

        for task in &mut self.tasks {
            if let Some(&new_state) = status_map.get(task.subject.as_str()) {
                task.status = new_state;
            }
        }
    }

    /// Format plan for user presentation.
    pub fn present_to_user(&self) -> String {
        let mut s = format!("## Plan: {}\n\n", self.objective);
        let (done, total) = self.progress();

        if !self.expected_files.is_empty() {
            s.push_str("### Files\n");
            for f in &self.expected_files { s.push_str(&format!("- {}\n", f)); }
            s.push('\n');
        }

        if !self.tasks.is_empty() {
            s.push_str(&format!("### Tasks ({}/{})\n", done, total));
            for t in &self.tasks {
                let icon = match t.status {
                    TaskState::Completed => "[x]", TaskState::InProgress => "[>]",
                    TaskState::Blocked => "[!]", TaskState::Pending => "[ ]",
                };
                let deps = if t.depends_on.is_empty() { String::new() }
                    else { format!(" (needs: {})", t.depends_on.join(", ")) };
                s.push_str(&format!("{} {} — {}{}\n", icon, t.subject, t.description, deps));
            }
            s.push('\n');
        }

        if !self.constraints.is_empty() {
            s.push_str("### Constraints (DO NOT CHANGE)\n");
            for c in &self.constraints { s.push_str(&format!("- {}\n", c)); }
            s.push('\n');
        }

        if !self.acceptance_criteria.is_empty() {
            s.push_str("### Acceptance\n");
            for a in &self.acceptance_criteria {
                s.push_str(&format!("- {}: `{}` (exit {})\n", a.description, a.verification_command, a.expected_exit_code));
            }
            s.push('\n');
        }

        s
    }

    /// Generate verification instructions for the Evaluator.
    pub fn evaluator_checklist(&self) -> String {
        let mut s = "## Evaluator Verification\n\n".to_string();
        s.push_str(&format!("Objective: {}\n\n", self.objective));
        s.push_str("### Task Completion\n");
        for t in &self.tasks {
            let ok = if t.status == TaskState::Completed { "done" } else { "MISSING" };
            s.push_str(&format!("- {}: {}\n", t.subject, ok));
        }
        s.push_str("\n### Acceptance Criteria\n");
        for a in &self.acceptance_criteria {
            s.push_str(&format!("- `{}` (expect exit {})\n", a.verification_command, a.expected_exit_code));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_plan_json() {
        let json = serde_json::json!({
            "objective": "Fix auth bug",
            "files": ["src/auth.rs"],
            "tasks": [{"subject": "Add null check", "description": "In login handler"}],
            "acceptance": [{"description": "Login works", "command": "cargo test", "expected_exit": 0}],
            "constraints": ["Don't change API"]
        });
        let contract = SprintContract::from_plan_json(&json).unwrap();
        assert_eq!(contract.objective, "Fix auth bug");
        assert_eq!(contract.tasks.len(), 1);
        assert_eq!(contract.acceptance_criteria.len(), 1);
        assert!(!contract.is_complete());
    }

    #[test]
    fn test_sync_todos() {
        let mut c = SprintContract::new("Test".into());
        c.tasks = vec![
            PlanTask { subject: "Task A".into(), description: "".into(), depends_on: vec![], status: TaskState::Pending },
            PlanTask { subject: "Task B".into(), description: "".into(), depends_on: vec!["Task A".into()], status: TaskState::Pending },
        ];
        let todos = vec![("Task A".into(), TodoStatus::Completed), ("Task B".into(), TodoStatus::InProgress)];
        c.sync_todos(&todos);
        assert_eq!(c.tasks[0].status, TaskState::Completed);
        assert_eq!(c.tasks[1].status, TaskState::InProgress);
    }

    #[test]
    fn test_progress() {
        let mut c = SprintContract::new("Test".into());
        c.tasks = vec![
            PlanTask { subject: "A".into(), description: "".into(), depends_on: vec![], status: TaskState::Completed },
            PlanTask { subject: "B".into(), description: "".into(), depends_on: vec![], status: TaskState::Pending },
        ];
        assert_eq!(c.progress(), (1, 2));
        assert!(!c.is_complete());
    }
}
