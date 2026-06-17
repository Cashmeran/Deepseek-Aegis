//! Output verification, goal checking, and graceful exit with summary.

use crate::agent::output::{AgentOutput, ConfidenceLevel, VerificationResult};
use crate::agent::system_prompt::HarnessPhase;
use crate::error::AgentResult;
use crate::llm::client::{LlmClient, LlmRequest};
use crate::types::message::Message;

use super::AgentLoop;

impl<L: LlmClient> AgentLoop<L> {
    /// Check if the active goal (SprintContract) is met using a cheap Flash call.
    pub(crate) async fn check_goal_completed(&self, latest_output: &str) -> Option<String> {
        let contract = self.active_contract.as_ref()?;
        if contract.acceptance_criteria.is_empty() { return None; }
        let criteria: Vec<String> = contract.acceptance_criteria.iter()
            .map(|c| c.description.clone()).collect();
        let criteria_str = criteria.join("; ");
        let prompt = format!(
            "Goal: {}\nSuccess criteria: {}\nLatest agent output: {}\n\nQuestion: Has the goal been fully achieved? Answer only YES or NO.",
            contract.objective, criteria_str,
            &latest_output[..latest_output.len().min(3000)]
        );
        let config = LlmRequest {
            model: "deepseek-v4-flash".into(),
            max_tokens: 10,
            temperature: 0.0,
            reasoning_effort: crate::types::tool::ReasoningEffort::Off,
            timeout: std::time::Duration::from_secs(30),
            user_id: self.config.user_id.clone(),
            thinking_enabled: false,
            strict_schema: false,
            web_search_enabled: false,
            tools_json: String::new(),
        };
        let messages = vec![Message::User(
            crate::types::message::UserMessage {
                id: "goal_check".into(),
                timestamp: chrono::Utc::now(),
                content: prompt,
                metadata: Default::default(),
            }
        )];
        match self.llm.chat("You are a goal verification assistant. Answer only YES or NO.", &messages, &config).await {
            Ok(resp) => {
                let answer = resp.content.unwrap_or_default().trim().to_uppercase();
                if answer.contains("YES") { Some("YES".into()) }
                else { Some("NO".into()) }
            }
            Err(_) => None,
        }
    }

    /// 验证代码输出。Multi-phase: heuristic checks + compiler diagnostics + test execution + git review.
    pub(crate) async fn verify_output(&mut self, content: &str) -> AgentResult<VerificationResult> {
        let mut blocking = 0u32;
        let mut advisory = 0u32;
        let mut details: Vec<String> = Vec::new();
        let workspace = std::path::PathBuf::from(&self.config.workspace_dir);

        // ── Phase 1: Heuristic checks ──
        if content.trim().is_empty() {
            blocking += 1;
            details.push("Empty output — no code or text produced".into());
        }

        if let Some(ref scorer) = self.code_scorer {
            let score = scorer.score("", content, "");
            if score < 0.4 {
                blocking += 1;
                details.push(format!("Quality score too low: {:.2} (threshold: 0.4)", score));
            } else if score < 0.7 {
                advisory += 1;
                details.push(format!("Quality score marginal: {:.2} (threshold: 0.7)", score));
            }
        }

        let todo_fixme_count = content.matches("TODO").count() + content.matches("FIXME").count();
        if todo_fixme_count > 0 {
            advisory += 1;
            details.push(format!("{} TODO/FIXME markers found", todo_fixme_count));
        }

        if content.contains("unsafe ") && !content.contains("unsafe {") {
            advisory += 1;
            details.push("Contains 'unsafe' keyword — verify safety invariants".into());
        }

        let unwrap_count = content.matches(".unwrap()").count();
        if unwrap_count > 3 {
            advisory += 1;
            details.push(format!("{} .unwrap() calls — consider proper error handling", unwrap_count));
        }

        // ── Phase 2: Compiler diagnostics ──
        let diag_output = std::process::Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(&workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        if let Ok(output) = diag_output {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let error_lines: Vec<&str> = stderr.lines()
                .filter(|l| l.contains("error") || l.contains("error:"))
                .take(10)
                .collect();
            if !error_lines.is_empty() {
                blocking += error_lines.len() as u32;
                for e in &error_lines[..error_lines.len().min(5)] {
                    details.push(format!("Compiler error: {}", e));
                }
                if error_lines.len() >= 5 {
                    details.push(format!("... and {} more errors", error_lines.len() - 5));
                }
            }

            let warn_lines: Vec<&str> = stderr.lines()
                .filter(|l| l.contains("warning") || l.contains("warning:"))
                .take(5)
                .collect();
            if !warn_lines.is_empty() {
                advisory += warn_lines.len() as u32;
                for w in &warn_lines[..warn_lines.len().min(3)] {
                    details.push(format!("Warning: {}", w));
                }
            }

            if !output.status.success() && error_lines.is_empty() {
                let truncated: String = stderr.lines().take(5).collect::<Vec<_>>().join("\n");
                blocking += 1;
                details.push(format!("cargo check FAILED:\n{}", truncated));
            }
        }

        // ── Phase 3: Test execution ──
        let test_output = std::process::Command::new("cargo")
            .args(["test", "--lib", "--no-fail-fast"])
            .current_dir(&workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        if let Ok(output) = test_output {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            let mut passed = 0u32;
            let mut failed = 0u32;
            for line in stderr.lines().chain(stdout.lines()) {
                if line.contains("test result:") {
                    for part in line.split(';') {
                        let part = part.trim();
                        if let Some(num) = part.split_whitespace().next().and_then(|n| n.parse::<u32>().ok()) {
                            if part.contains("passed") { passed = num; }
                            else if part.contains("failed") { failed = num; }
                        }
                    }
                }
            }

            if !output.status.success() || failed > 0 {
                blocking += failed;
                details.push(format!("Tests: {} passed, {} FAILED", passed, failed));

                let failures: Vec<&str> = stderr.lines()
                    .filter(|l| l.contains("FAILED") || l.contains("panicked") || l.contains("assertion"))
                    .take(8)
                    .collect();
                for f in &failures[..failures.len().min(5)] {
                    details.push(format!("Test failure: {}", f));
                }
            } else if passed > 0 {
                details.push(format!("Tests: {} passed, 0 failed", passed));
            }
        }

        // ── Phase 4: Git change review ──
        let git_diff = std::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        if let Ok(output) = git_diff {
            let stat = String::from_utf8_lossy(&output.stdout);
            if !stat.trim().is_empty() {
                let changed_files: Vec<&str> = stat.lines()
                    .filter(|l| l.contains('|'))
                    .take(10)
                    .collect();
                let file_count = changed_files.len();
                details.push(format!("Files changed: {}", file_count));
                if file_count > 5 {
                    advisory += 1;
                    details.push("More than 5 files modified — verify all changes are intended".into());
                }
                for cf in &changed_files {
                    let path = cf.split('|').next().unwrap_or("").trim();
                    if path.contains("Cargo.lock") || path.contains(".gitignore") {
                        advisory += 1;
                        details.push(format!("Sensitive file changed: {}", path));
                    }
                }
            }
        }

        // ── Phase 5: SprintContract check ──
        if let Some(ref contract) = self.active_contract {
            if !contract.is_complete() {
                let (done, total) = contract.progress();
                blocking += 1;
                details.push(format!("Contract: {}/{} tasks incomplete", total - done, total));
            }

            let blocked = contract.blocked_tasks();
            if !blocked.is_empty() {
                advisory += 1;
                details.push(format!("{} tasks blocked by dependencies", blocked.len()));
            }

            for criterion in &contract.acceptance_criteria {
                // P2: Machine-verifiable exit — actually run the verification command
                if !criterion.verification_command.is_empty() {
                    match Self::run_acceptance_cmd(&criterion.verification_command) {
                        Ok((exit_code, stdout, stderr)) => {
                            let passed = exit_code == criterion.expected_exit_code;
                            let output = format!("{}{}", stdout, stderr);
                            if let Some(ref expected) = criterion.expected_output_contains {
                                if !output.contains(expected) {
                                    if !passed { blocking += 1; } else { advisory += 1; }
                                    details.push(format!(
                                        "Acceptance '{}': cmd `{}` exit={} (expected {}), \
                                         expected '{}' not found in output",
                                        criterion.description, criterion.verification_command,
                                        exit_code, criterion.expected_exit_code, expected
                                    ));
                                    continue;
                                }
                            }
                            if passed {
                                details.push(format!(
                                    "✅ Acceptance '{}': `{}` passed (exit {})",
                                    criterion.description, criterion.verification_command, exit_code
                                ));
                            } else {
                                blocking += 1;
                                details.push(format!(
                                    "Acceptance '{}': `{}` exit={} (expected {})",
                                    criterion.description, criterion.verification_command,
                                    exit_code, criterion.expected_exit_code
                                ));
                            }
                        }
                        Err(e) => {
                            blocking += 1;
                            details.push(format!(
                                "Acceptance '{}': `{}` failed to run — {}",
                                criterion.description, criterion.verification_command, e
                            ));
                        }
                    }
                } else if let Some(ref expected) = criterion.expected_output_contains {
                    // Fallback: check output text for expected string (no command defined)
                    if !content.contains(expected) {
                        advisory += 1;
                        details.push(format!(
                            "Acceptance '{}': expected '{}' not found in output",
                            criterion.description, expected
                        ));
                    }
                }
            }

            self.phase = HarnessPhase::Evaluator;
        }

        if blocking > 0 {
            Ok(VerificationResult::failed(
                blocking,
                advisory,
                details.join("\n"),
                format!("FAIL: {} blocking, {} advisory issues", blocking, advisory),
            ))
        } else if advisory > 0 {
            Ok(VerificationResult::passed(format!(
                "PASS with {} advisory issues:\n{}",
                advisory,
                details.join("\n")
            )))
        } else {
            Ok(VerificationResult::passed(
                "All checks passed — no issues found.".into()
            ))
        }
    }

    /// Run an acceptance criterion's verification command and return (exit_code, stdout, stderr).
    fn run_acceptance_cmd(command: &str) -> Result<(i32, String, String), String> {
        let output = std::process::Command::new(if cfg!(windows) { "powershell" } else { "bash" })
            .arg(if cfg!(windows) { "-Command" } else { "-c" })
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| format!("{}", e))?;
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok((exit_code, stdout, stderr))
    }

    /// 上下文溢出时退出并总结。
    pub(crate) async fn exit_with_summary(&self) -> AgentResult<AgentOutput> {
        let force_prompt = crate::agent::healing::force_summary_prompt();
        let config = LlmRequest {
            model: "deepseek-v4-flash".into(),
            max_tokens: 300,
            temperature: 0.0,
            reasoning_effort: crate::types::tool::ReasoningEffort::Off,
            timeout: std::time::Duration::from_secs(15),
            user_id: self.config.user_id.clone(),
            thinking_enabled: false,
            strict_schema: false,
            web_search_enabled: false,
            tools_json: String::new(),
        };
        let messages = vec![Message::User(crate::types::message::UserMessage {
            id: "force_summary".into(),
            timestamp: chrono::Utc::now(),
            content: force_prompt,
            metadata: Default::default(),
        })];

        match self.llm.chat("Be brief.", &messages, &config).await {
            Ok(resp) => {
                let summary = resp.content.unwrap_or_default();
                Ok(AgentOutput {
                    content: format!("Context nearly full. Forced summary:\n\n{}", summary),
                    confidence: ConfidenceLevel::Low,
                    verification_report: None,
                    summary: Some("Context overflow — forced summary".into()),
                })
            }
            Err(_) => Ok(AgentOutput {
                content: "Context nearly full. Use /clear to free space or start a new session.".into(),
                confidence: ConfidenceLevel::Low,
                verification_report: None,
                summary: Some("Context overflow — task incomplete".into()),
            }),
        }
    }
}
