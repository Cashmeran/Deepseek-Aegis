//! Free helper functions for the AgentLoop.
//! Task classification, retry-ability checks, scoring utilities.

use crate::types::tool::TaskType;

/// 三体系统: 判断任务是否复杂，决定是否进入 Planner 阶段。
/// 复杂任务标准: 3+ 步骤 / 2+ 文件 / 新功能 / 重构 / 跨模块修改。
pub(crate) fn is_complex_task(user_input: &str) -> bool {
    let lower = user_input.to_lowercase();
    let complex_keywords = [
        "implement", "create", "build", "refactor", "migrate",
        "design", "architect", "restructure", "rewrite", "scaffold",
        "实现", "创建", "构建", "重构", "迁移", "设计", "架构",
        "module", "feature", "system", "pipeline",
    ];
    let file_count = lower.matches("file").count()
        + lower.matches("文件").count()
        + lower.matches("crate").count()
        + lower.matches("module").count()
        + lower.matches("模块").count();
    let step_count = lower.matches(',').count()
        + lower.matches("step").count()
        + lower.matches("步骤").count()
        + lower.matches("first").count()
        + lower.matches("then").count()
        + lower.matches("首先").count()
        + lower.matches("然后").count();

    let has_complex_keyword = complex_keywords.iter().any(|k| lower.contains(k));
    has_complex_keyword || file_count >= 2 || step_count >= 2
}

/// 根据用户输入和 LLM 输出分类任务类型。
pub(crate) fn classify_task_type(user_input: &str, output: &str) -> TaskType {
    let combined = format!("{} {}", user_input, output);
    let lower = combined.to_lowercase();

    let gen_keywords = ["implement", "create", "build", "generate", "write a", "scaffold"];
    let edit_keywords = ["fix", "refactor", "change", "update", "modify", "edit", "edited", "editing", "remove"];
    let question_keywords = ["what", "how", "why", "explain"];

    if question_keywords
        .iter()
        .any(|k| lower.starts_with(k) || lower.contains(&format!(" {}", k)))
        || lower.contains('?')
    {
        return TaskType::Question;
    }

    if gen_keywords.iter().any(|k| lower.contains(k)) {
        return TaskType::CodeGeneration;
    }

    if edit_keywords
        .iter()
        .any(|k| is_word_match(k, &lower))
    {
        return TaskType::CodeEdit;
    }

    if output.contains("```") {
        return TaskType::CodeEdit;
    }

    TaskType::Conversation
}

/// 检查 keyword 是否作为独立词出现在 text 中。
/// 词边界: keyword 前必须是非字母数字或开头, 后必须是非字母数字或结尾。
pub(crate) fn is_word_match(keyword: &str, text: &str) -> bool {
    if let Some(pos) = text.find(keyword) {
        let before = pos == 0 || {
            let c = text.as_bytes()[pos - 1];
            !c.is_ascii_alphanumeric()
        };
        let after = pos + keyword.len() >= text.len() || {
            let c = text.as_bytes()[pos + keyword.len()];
            !c.is_ascii_alphanumeric()
        };
        before && after
    } else {
        false
    }
}

/// 将 ConfidenceScorer 的原始 0.0-1.0 分数映射到 ConfidenceLevel。
pub(crate) fn score_to_level(raw_score: f32) -> crate::agent::output::ConfidenceLevel {
    if raw_score >= 0.9 {
        crate::agent::output::ConfidenceLevel::High
    } else if raw_score >= 0.6 {
        crate::agent::output::ConfidenceLevel::Medium
    } else {
        crate::agent::output::ConfidenceLevel::Low
    }
}

/// 判断 LLM 错误是否可重试。
/// 4xx 请求错误不重试 (客户端错误), 5xx/网络错误可重试。
pub(crate) fn is_retryable(err: &crate::error::AgentError) -> bool {
    match err {
        crate::error::AgentError::ApiUnreachable { .. } => true,
        crate::error::AgentError::RateLimited { .. } => true,
        crate::error::AgentError::ApiError { status, .. } => *status >= 500,
        _ => false,
    }
}

pub(crate) fn parse_reasoning_effort(s: &str) -> crate::types::tool::ReasoningEffort {
    match s.to_lowercase().as_str() {
        "off" => crate::types::tool::ReasoningEffort::Off,
        "high" => crate::types::tool::ReasoningEffort::High,
        "max" => crate::types::tool::ReasoningEffort::Max,
        _ => crate::types::tool::ReasoningEffort::Max,
    }
}
