//! LLM call with Pass@3 error recovery and monotonic budget decay.

use crate::agent::helpers;
use crate::error::{AgentError, AgentResult};
use crate::llm::client::{LlmClient, LlmRequest, LlmResponse};

use super::AgentLoop;

impl<L: LlmClient> AgentLoop<L> {
    /// 调用 LLM (带 Pass@3 错误恢复)。
    /// 非可重试错误 (400/401/402/403/404/405/422) 不重试。
    pub(crate) async fn call_llm_with_recovery(&mut self, user_input: &str) -> AgentResult<LlmResponse> {
        let mut last_err = None;
        let mut budget_ratio = 1.0f32;

        let system = self.build_system_prompt(user_input);
        let messages = self.conversation().messages();

        for attempt in 0..self.config.retry_max_attempts {
            let tools_json = self.registry.get_anthropic_tools_json();
            let request = LlmRequest {
                model: self.config.default_model.clone(),
                max_tokens: (393_216.0 * budget_ratio) as u32,
                temperature: 0.0,
                reasoning_effort: helpers::parse_reasoning_effort(&self.config.reasoning_effort),
                timeout: std::time::Duration::from_secs(120),
                user_id: self.config.user_id.clone(),
                thinking_enabled: self.config.thinking_enabled,
                strict_schema: self.config.strict_tool_schema,
                web_search_enabled: self.config.web_search_enabled,
                tools_json,
            };

            match self.llm.chat(&system, messages, &request).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if !helpers::is_retryable(&e) {
                        return Err(e);
                    }
                    last_err = Some(e);
                    budget_ratio *= 0.6;
                    if (attempt as usize) < self.config.retry_max_attempts as usize - 1 {
                        let delay_ms =
                            self.config.retry_initial_backoff_ms * 2u64.pow(attempt);
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| AgentError::Internal("Max retries exceeded".into())))
    }
}
