//! Sub-agent system — CC AgentTool pattern with light Swarm Engine additions.
//!
//! Architecture:
//!   AgentDefinition (config) → AgentTool (tool) → SubagentRunner → AgentLoop (existing)
//!
//! Built-in agents: Explore (read-only fast search), Plan (plan-only), GeneralPurpose (full tools).
//! Custom agents: loaded from `.aegis/agents/*.md` (YAML frontmatter + markdown body).

use std::path::Path;

// ═══════════════ AgentDefinition ═══════════════

/// Definition of a sub-agent type. Aligned with CC AgentJsonSchema.
#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub system_prompt: String,
    /// Allowlist — None means all tools. Empty vec means no tools.
    pub tools: Option<Vec<String>>,
    /// Denylist — these tools are removed from the agent's available tools.
    pub disallowed_tools: Vec<String>,
    /// Model override — None means inherit from parent.
    pub model: Option<String>,
    /// Max agent turns — None means default (50 for sub-agents).
    pub max_turns: Option<u32>,
    /// Source: builtin, project (.aegis/agents/), user (~/.aegis/agents/)
    pub source: AgentSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSource {
    Builtin,
    Project(String), // file path
    User(String),    // file path
}

// ═══════════════ SubagentResult ═══════════════

/// Structured result from a sub-agent execution (Swarm Engine MemberResult pattern).
#[derive(Debug, Clone)]
pub struct SubagentResult {
    pub agent_name: String,
    pub output: String,
    pub tokens_used: u64,
    pub elapsed_ms: u64,
    pub error: Option<String>,
    pub model: String,
}

// ═══════════════ Built-in agents ═══════════════

///  Explore agent: read-only, fast, file search specialist.
pub fn explore_agent() -> AgentDefinition {
    AgentDefinition {
        name: "Explore".into(),
        description: "Fast agent specialized for exploring codebases".into(),
        when_to_use: "Use this when you need to quickly find files by patterns (eg. \"src/components/**/*.tsx\"), search code for keywords (eg. \"API endpoints\"), or answer questions about the codebase (eg. \"how do API endpoints work?\"). When calling this agent, specify the desired thoroughness level: \"quick\" for basic searches, \"medium\" for moderate exploration, or \"very thorough\" for comprehensive analysis across multiple locations and naming conventions.".into(),
        system_prompt: "You are a file search specialist for aegis. You excel at thoroughly navigating and exploring codebases.\n\n\
=== CRITICAL: READ-ONLY MODE - NO FILE MODIFICATIONS ===\n\
This is a READ-ONLY exploration task. You are STRICTLY PROHIBITED from:\n\
- Creating new files (no Write, touch, or file creation of any kind)\n\
- Modifying existing files (no Edit operations)\n\
- Deleting files (no rm or deletion)\n\
- Moving or copying files (no mv or cp)\n\
- Running ANY commands that change system state\n\n\
Your role is EXCLUSIVELY to search and analyze existing code.\n\n\
Your strengths:\n\
- Rapidly finding files using glob patterns\n\
- Searching code and text with powerful regex patterns\n\
- Reading and analyzing file contents\n\n\
Guidelines:\n\
- Use Glob for broad file pattern matching\n\
- Use Grep for searching file contents with regex\n\
- Use file_read when you know the specific file path\n\
- Use Bash ONLY for read-only operations (ls, git status, git log, git diff, cat, head, tail)\n\
- NEVER use Bash for: mkdir, touch, rm, cp, mv, git add, git commit, npm install, pip install\n\
- Adapt your search approach based on the thoroughness level specified\n\
- Communicate your final report directly — do NOT attempt to create files\n\n\
NOTE: You are meant to be a fast agent that returns output as quickly as possible.\n\
- Make efficient use of tools: be smart about how you search\n\
- Wherever possible, spawn multiple parallel tool calls for searching and reading\n\
- Complete the search request efficiently and report findings clearly.".into(),
        tools: Some(vec![
            "file_read".into(), "glob".into(), "grep".into(),
            "file_search".into(), "list_dir".into(), "bash".into(),
            "lsp".into(), "web_fetch".into(),
        ]),
        disallowed_tools: vec![
            "file_edit".into(), "file_write".into(), "apply_patch".into(),
            "run_tests".into(), "plan".into(), "todo_write".into(),
        ],
        model: Some("deepseek-v4-flash".into()),
        max_turns: Some(30),
        source: AgentSource::Builtin,
    }
}

///  Plan agent: read-only + plan tool, no writes.
pub fn plan_agent() -> AgentDefinition {
    AgentDefinition {
        name: "Plan".into(),
        description: "Software architect agent for designing implementation plans".into(),
        when_to_use: "Use this when you need to plan the implementation strategy for a task. Returns step-by-step plans, identifies critical files, and considers architectural trade-offs.".into(),
        system_prompt: "You are a software architect. Your job is to design implementation plans.\n\n\
=== READ-ONLY + PLAN ONLY ===\n\
You can READ files and CREATE PLANS. You CANNOT edit, write, or execute code.\n\
- Use file_read, glob, grep to survey the codebase\n\
- Use plan tool to create structured plans with objective/tasks/acceptance criteria\n\
- Output the plan — do NOT implement it\n\n\
Your plans should include:\n\
1. Clear objectives\n\
2. Step-by-step tasks with file paths\n\
3. Critical files to modify\n\
4. Architectural trade-offs considered\n\
5. Verification steps\n\n\
Be thorough but concise. The main agent will execute, not you.".into(),
        tools: Some(vec![
            "file_read".into(), "glob".into(), "grep".into(),
            "file_search".into(), "list_dir".into(), "plan".into(),
        ]),
        disallowed_tools: vec![
            "file_edit".into(), "file_write".into(), "bash".into(),
            "apply_patch".into(), "run_tests".into(),
        ],
        model: Some("deepseek-v4-flash".into()),
        max_turns: Some(20),
        source: AgentSource::Builtin,
    }
}

/// General-purpose agent with all tools, inherits parent model.
pub fn general_purpose_agent() -> AgentDefinition {
    AgentDefinition {
        name: "GeneralPurpose".into(),
        description: "General-purpose agent for researching complex questions, searching for code, and executing multi-step tasks.".into(),
        when_to_use: "When you are searching for a keyword or file and are not confident that you will find the right match in the first few tries, use this agent to perform the search for you.".into(),
        system_prompt: "You are a general-purpose assistant for aegis. Complete the assigned task efficiently.\n\n\
- Use all available tools as needed\n\
- Be thorough but efficient\n\
- Report results clearly and concisely\n\
- The main agent is waiting for your output — don't ask follow-up questions".into(),
        tools: None, // all tools
        disallowed_tools: vec![],
        model: None, // inherit
        max_turns: Some(50),
        source: AgentSource::Builtin,
    }
}

/// All built-in agent definitions.
pub fn builtin_agents() -> Vec<AgentDefinition> {
    vec![explore_agent(), general_purpose_agent()]
}

// ═══════════════ Custom agent loading ═══════════════

/// Load custom agent definitions from `.aegis/agents/*.md`.
/// Format: YAML frontmatter + markdown body, .
pub fn load_agents_dir(base_dir: &Path) -> Vec<AgentDefinition> {
    let agents_dir = base_dir.join(".aegis").join("agents");
    if !agents_dir.is_dir() {
        return vec![];
    }

    let mut agents = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            if let Some(agent) = parse_agent_md(&path) {
                agents.push(agent);
            }
        }
    }

    // Also check user-level ~/.aegis/agents/
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
    {
        let user_dir = home.join(".aegis").join("agents");
        if user_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&user_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().is_none_or(|e| e != "md") {
                        continue;
                    }
                    if let Some(mut agent) = parse_agent_md(&path) {
                        agent.source = AgentSource::User(path.to_string_lossy().to_string());
                        agents.push(agent);
                    }
                }
            }
    }

    agents
}

/// Parse a single agent .md file with YAML frontmatter.
fn parse_agent_md(path: &Path) -> Option<AgentDefinition> {
    let raw = std::fs::read_to_string(path).ok()?;
    let content = raw.trim_start();

    // Parse YAML frontmatter between --- markers
    let (fm, body) = if content.starts_with("---\n") || content.starts_with("---\r\n") {
        let after = &content[4..];
        {
            let end = after.find("\n---")?;
            (&after[..end], after[end + 4..].trim())
        }
    } else {
        return None; // require frontmatter
    };

    let name = parse_yaml_field(fm, "name")?;
    let description = parse_yaml_field(fm, "description").unwrap_or_else(|| "".into());
    let when_to_use = parse_yaml_field(fm, "when-to-use")
        .or_else(|| parse_yaml_field(fm, "when_to_use"))
        .unwrap_or_else(|| "".into());
    let tools: Option<Vec<String>> = parse_yaml_list(fm, "tools");
    let disallowed_tools: Vec<String> = parse_yaml_list(fm, "disallowed-tools")
        .or_else(|| parse_yaml_list(fm, "disallowed_tools"))
        .unwrap_or_default();
    let model = parse_yaml_field(fm, "model");
    let max_turns: Option<u32> = parse_yaml_field(fm, "max-turns")
        .or_else(|| parse_yaml_field(fm, "max_turns"))
        .and_then(|s| s.parse().ok());

    let file_path = path.to_string_lossy().to_string();

    Some(AgentDefinition {
        name,
        description,
        when_to_use,
        system_prompt: body.to_string(),
        tools,
        disallowed_tools,
        model,
        max_turns,
        source: AgentSource::Project(file_path),
    })
}

fn parse_yaml_field(fm: &str, key: &str) -> Option<String> {
    for line in fm.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':')
            && k.trim() == key {
                let val = v.trim().trim_matches('"').trim();
                if val.is_empty() { return None; }
                return Some(val.to_string());
            }
    }
    None
}

fn parse_yaml_list(fm: &str, key: &str) -> Option<Vec<String>> {
    for line in fm.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':')
            && k.trim() == key {
                let v = v.trim();
                if v.starts_with('[') && v.ends_with(']') {
                    let inner = &v[1..v.len()-1];
                    return Some(inner.split(',')
                        .map(|s| s.trim().trim_matches('"').trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect());
                }
            }
    }
    None
}

/// Find an agent definition by name (case-insensitive). Built-in first, then custom.
pub fn find_agent<'a>(name: &str, builtins: &'a [AgentDefinition], customs: &'a [AgentDefinition]) -> Option<&'a AgentDefinition> {
    let lower = name.to_lowercase();
    builtins.iter().chain(customs).find(|a| a.name.to_lowercase() == lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_agents_count() {
        let agents = builtin_agents();
        assert_eq!(agents.len(), 2);
    }

    #[test]
    fn test_explore_agent_readonly() {
        let agent = explore_agent();
        assert_eq!(agent.name, "Explore");
        assert!(agent.tools.is_some());
        // Explore should NOT have file_edit or file_write
        let tools = agent.tools.as_ref().unwrap();
        assert!(!tools.contains(&"file_edit".to_string()));
        assert!(!tools.contains(&"file_write".to_string()));
        // Explore SHOULD have file_read and glob
        assert!(tools.contains(&"file_read".to_string()));
        assert!(tools.contains(&"glob".to_string()));
    }

    #[test]
    fn test_plan_agent_no_bash() {
        let agent = plan_agent();
        assert!(agent.disallowed_tools.contains(&"bash".to_string()));
    }

    #[test]
    fn test_find_agent_case_insensitive() {
        let builtins = builtin_agents();
        let customs = vec![];
        assert!(find_agent("explore", &builtins, &customs).is_some());
        assert!(find_agent("EXPLORE", &builtins, &customs).is_some());
        assert!(find_agent("nonexistent", &builtins, &customs).is_none());
    }

    #[test]
    fn test_parse_agent_md() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("aegis_agent_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test-agent.md");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"---\nname: test-agent\ndescription: Test agent\nwhen-to-use: For testing\ntools: [file_read, glob]\nmodel: deepseek-v4-flash\nmax-turns: 10\n---\n\nYou are a test agent.").unwrap();

        let agent = parse_agent_md(&path).unwrap();
        assert_eq!(agent.name, "test-agent");
        assert_eq!(agent.description, "Test agent");
        assert!(agent.system_prompt.contains("You are a test agent"));
        assert_eq!(agent.tools.unwrap(), vec!["file_read", "glob"]);
        assert_eq!(agent.model.unwrap(), "deepseek-v4-flash");
        assert_eq!(agent.max_turns.unwrap(), 10);
    }
}
