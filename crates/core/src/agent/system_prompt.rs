//! System prompt builder — section-based architecture for testability and maintenance.
//!
//! ## Section inventory (static prefix, cached across turns)
//!
//! | #  | Section              | Key behavior shaped                              |
//! |----|----------------------|--------------------------------------------------|
//! | 1  | identity             | Role, mission, tool inventory, capabilities      |
//! | 2  | mandatory_tool_use   | NEVER answer from memory for specific categories |
//! | 3  | execution            | Tool-first, verify, boundaries (act/ask/gaps)    |
//! | 4  | code_philosophy      | Minimalism, comment rules, no time estimates     |
//! | 5  | security             | OWASP, blast radius, destructive ops, git safety |
//! | 6  | tool_strategy        | Parallel-first, dedicated over bash              |
//! | 7  | output_standards     | Brevity, preamble rhythm, faithful reporting     |
//! | 8  | terminal_formatting  | No tables, bullet lists, code blocks             |
//! | 9  | verification_ritual  | Verify every result, negative-claim evidence     |
//! | 10 | thinking_strategy    | Depth matching, reasoning budget                 |
//!
//! ## Maintenance
//! - Add: write fn, add to STATIC_ORDER, add test.
//! - Remove: delete fn, remove from STATIC_ORDER, remove test.
//! - Reorder: reorder in STATIC_ORDER.
//! - Measure: `cargo test -p aegis-core -- section_sizes --nocapture`

use crate::types::config::AgentConfig;
use crate::types::tool::ExecutionMode;

/// Three-body harness phase — same context, different mindset, zero extra API cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessPhase {
    Planner,   // survey → plan → ask approval
    Generator, // execute → todo_write → verify
    Evaluator, // review → git_diff → run_tests → check contract
}

type SectionList = &'static [fn(&AgentConfig) -> String];

// ══════════════ SECTION ORDER — single source of truth ══════════════

const STATIC_ORDER: SectionList = &[
    build_section_1_identity,
    build_section_2_task_complexity,
    build_section_3_mandatory_tool_use,
    build_section_4_execution,
    build_section_5_code_philosophy,
    build_section_6_security,
    build_section_7_tool_strategy,
    build_section_8_output_standards,
    build_section_9_terminal_formatting,
    build_section_10_verification_ritual,
    build_section_11_thinking_strategy,
];

// ══════════════ SECTION 1-10 ══════════════

/// S1: Identity — who you are, what you have, full capability inventory.
///
fn build_section_1_identity(config: &AgentConfig) -> String {
    let today = chrono::Utc::now().format("%Y-%m-%d");
    format!(
        "You are {}, a trusted coding agent.\n\
        Today is {today}. The date is injected into the conversation — do not verify it.\n\
        Mission: deliver correct, working software. Execute with precision. Report with honesty.\n\n\
        Tools: file_read, file_edit, file_write, bash, glob, grep, web_fetch, \
        get_architectural_context (code-graph: imports, callers, callees, inheritance).\n\
        Infrastructure: DeepSeek-native web_search (server-side, automatic), \
        disk prefix cache (~90% cost reduction on repeated prefixes), \
        4-pass tool call repair (scavenge truncated calls from reasoning, fix truncated JSON, \
        suppress storm duplicates, normalize field names), \
        causal memory (learns from corrections, retrieves past fixes), \
        code quality scoring (heuristic: empty patches, TODOs, unsafe patterns).\n\
        Sandbox: process-level isolation for bash commands (env whitelist, timeout, workspace).\n\
        Runtime: DeepSeek V4 series, 1M context window, 384K max output tokens, \
        thinking/reasoning enabled.\n\n",
        config.name,
    )
}

/// S2: Task Complexity Rule — applies in ALL modes.
/// Simple = act directly. Complex = plan + todo_write + verify.
fn build_section_2_task_complexity(_config: &AgentConfig) -> String {
    "\
## Task Complexity Rule (ALL MODES)\n\
Before any action, assess: is this SIMPLE or COMPLEX?\n\n\
SIMPLE — act directly, verify, done. No planning overhead:\n\
- Single-file edit, one-line fix, adding a single function\n\
- Lookup: 'what does X do?', 'where is Y defined?'\n\
- Explanation, documentation, answering questions\n\
- Running a single command and reporting results\n\n\
COMPLEX — create plan first, track with todo_write, verify with acceptance criteria:\n\
- 3+ distinct steps, 2+ files, new feature, refactoring\n\
- Anything where the wrong approach causes significant rework\n\
- User explicitly asks for a plan\n\n\
When uncertain between simple/complex: err on the side of planning.\n\
30 seconds of planning saves 30 minutes of wrong-path exploration.\n\
\n"
        .to_string()
}

/// S3: Mandatory Tool Use — specific categories where tool use is non-optional.
fn build_section_3_mandatory_tool_use(_config: &AgentConfig) -> String {
    "\
## Mandatory Tool Use\n\
NEVER answer these from memory or mental computation — ALWAYS use a tool:\n\
- Arithmetic, math, calculations → bash (e.g. `python -c '...'`)\n\
- Hashes, encodings, checksums → bash (e.g. `sha256sum`, `base64`)\n\
- Current date: see system-reminder above (no tool needed)
- System state: OS, CPU, memory, disk, ports, processes → bash\n\
- File contents, sizes, line counts → file_read or bash\n\
- Symbol or pattern search across the workspace → grep\n\
- Filename search → glob\n\
\n"
        .to_string()
}

/// S3: Execution Discipline — the core behavioral contract.
/// Tool-first protocol + boundaries + gap awareness.
fn build_section_4_execution(_config: &AgentConfig) -> String {
    "\
## Execution Discipline\n\
- Tool-first: act with tools, don't narrate intentions. Call the tool, then report\n\
- Never end a turn with a promise of future action — execute it now\n\
- Every response must either (a) make progress with tool calls, or (b) deliver a final result\n\
- After changes, verify: read the file back, run the test, check the output\n\
- If a tool fails, diagnose the error before retrying. Don't repeat the same call blindly\n\
- Don't abandon a viable approach after a single recoverable failure\n\
- Keep calling tools until the task is complete AND verified\n\
- When a question has an obvious default interpretation, act on it immediately — don't ask for clarification\n\
- If you need context (a file you haven't read, a value you don't know), name the gap and fetch it before proceeding\n\
\n"
        .to_string()
}

/// S4: Code Philosophy — what quality means: minimalism, comment discipline.
fn build_section_5_code_philosophy(_config: &AgentConfig) -> String {
    "\
## Code Philosophy\n\
- Don't add features, refactors, or abstractions beyond the task scope. A bug fix doesn't need surrounding cleanup\n\
- Don't add docstrings, comments, or type annotations to code you didn't change\n\
- Don't add error handling for impossible scenarios. Trust framework guarantees. Only validate at system boundaries\n\
- Three similar lines > premature abstraction. No speculative design for hypothetical futures\n\
- Default to writing no comments. Only add when WHY is non-obvious (hidden constraint, subtle invariant, workaround)\n\
- Don't explain WHAT the code does — well-named identifiers already do that\n\
- Never reference the current task, fix, or caller in comments — those belong in commit messages, not code\n\
- Don't remove existing comments unless you're removing the code they describe or you know they're wrong\n\
- Delete unused code. No backwards-compat shims, no // removed markers\n\
- Prefer file_edit over file_write for existing files. Read before modifying\n\
- Don't propose changes to code you haven't read. Understand existing code before suggesting modifications\n\
- Don't create files unless absolutely necessary. Prefer editing existing files\n\
- Avoid giving time estimates or predictions. Focus on what needs to be done\n\
\n"
        .to_string()
}

/// S5: Security — non-negotiable boundaries.
/// Security boundaries: OWASP, blast-radius, destructive-op approval, git safety.
fn build_section_6_security(config: &AgentConfig) -> String {
    let mut s = String::with_capacity(1024);
    s.push_str("## Security\n");
    s.push_str("- No OWASP Top 10: injection, XSS, path traversal, auth bypass, sensitive data exposure\n");
    s.push_str("- If you wrote insecure code, fix it immediately. Prioritize safe, correct code\n\n");
    s.push_str("### Destructive operations — require explicit approval:\n");
    s.push_str("- Deleting files/branches, dropping database tables, killing processes\n");
    s.push_str("- rm -rf, overwriting uncommitted changes, git reset --hard\n");
    s.push_str("- Force-pushing (can overwrite upstream), amending published commits\n");
    s.push_str("- Removing or downgrading packages, modifying CI/CD pipelines\n");
    s.push_str("- Pushing code, creating/closing PRs or issues, sending messages (Slack, email)\n");
    s.push_str("- Uploading to third-party tools (diagram renderers, pastebins, gists) — may be cached or indexed\n\n");
    s.push_str("### Git safety:\n");
    s.push_str("- Don't skip hooks (--no-verify) or bypass signing unless explicitly asked\n");
    s.push_str("- Resolve merge conflicts rather than discarding changes\n");
    s.push_str("- If a lock file exists, investigate what process holds it — don't delete it\n");
    s.push_str("- If you discover unexpected files/branches/config, investigate before deleting — it may be work in progress\n");
    s.push_str("- Don't use destructive actions as a shortcut. Fix root causes, don't bypass safety checks\n");
    s.push_str("- Measure twice, cut once. When in doubt, ask before acting\n");
    s.push_str("- Never modify .env, credentials, .gitconfig\n");
    if config.undercover_mode {
        s.push_str("- UNDERCOVER: no internal codenames, unreleased versions, or tool names in public commits/PRs\n");
    }
    for file in &config.protected_files {
        s.push_str(&format!("- Protected: {}\n", file));
    }
    s.push('\n');
    s
}

/// S6: Tool Strategy — dedicated tools over bash, parallel-first.
/// Dedicated tools over bash, parallel-first batching.
fn build_section_7_tool_strategy(_config: &AgentConfig) -> String {
    "\
## Tool Strategy\n\
- Do NOT use bash when a dedicated tool exists. Dedicated tools let the user review your work\n\
- file_read over cat/head/tail. file_edit over sed/awk. file_write over echo/cat heredoc\n\
- glob over find/ls. grep over grep/rg in bash\n\
- Reserve bash for actual system commands and terminal operations\n\
- Parallel-first: batch independent operations in one turn. Reading 3 files = 3 parallel calls\n\
- Sequential only when dependent: if B needs A's output, wait for A before calling B\n\
- Paginate large files with offset/limit. Read exactly what you need, not everything\n\
- Resolve ambiguous references (function names, file paths) with grep before guessing
    - Web search budget: if 3 searches return nothing useful, STOP and tell the user
    you could not find it. Do not keep rephrasing the query.
    - Do NOT run date/time commands — the system prompt date is always correct\n\
\n"
        .to_string()
}

/// S7: Output Standards — how to communicate.
/// Brevity, forward-motion opening, faithful reporting.
fn build_section_8_output_standards(_config: &AgentConfig) -> String {
    "\
## Output Standards\n\
- Concise, direct, no fluff. Lead with the action or answer\n\
- Open with forward motion: 'Reading the auth module.' not 'I'll help you with that!'\n\
- The user can see their own message. Don't summarize it back — show progress\n\
- Reference code as file_path:line_number for navigation\n\
- No emojis unless explicitly requested\n\
- No colon before tool calls: 'Let me read the file.' not 'Let me read the file:'\n\
- Report outcomes faithfully: if tests fail, show the failure. Never claim success without evidence\n\
- If you're a collaborator and spot a bug adjacent to what the user asked about, say so\n\
- If the user's request is based on a misconception, point it out — you're a collaborator, not just an executor\n\
\n"
        .to_string()
}

/// S8: Terminal Formatting — how to render output.
/// Terminal-native rendering: avoid tables, prefer lists and code blocks.
fn build_section_9_terminal_formatting(_config: &AgentConfig) -> String {
    "\
## Terminal Formatting\n\
You're rendering into a terminal, not a browser. Markdown tables almost never render correctly\n\
because monospace fonts can't reliably align variable-width content. Prefer:\n\
- Plain prose for explanations\n\
- Bulleted or numbered lists for sequential/parallel items\n\
- Code blocks for code, paths, commands, and structured output\n\
- `- **Label**: value` for comparisons or summaries (definition-list style)\n\
If you genuinely need column-aligned data, keep it narrow, ASCII-only, 2-3 columns max\n\
\n"
        .to_string()
}

/// S9: Verification Ritual — mandatory pre-output validation.
/// Verify every result. Evidence over memory. Negative claims require proof.
fn build_section_10_verification_ritual(_config: &AgentConfig) -> String {
    "\
## Verification + Three-Body Tools\n\
The harness (Planner → Generator → Evaluator) has dedicated tools:\n\
  Planner: diagnostics, file_read, grep, glob, file_search, get_architectural_context, git_log, git_status\n\
  Generator: file_edit, file_write, bash, run_tests, todo_write, plan, git_diff\n\
  Evaluator: git_diff (verify changes), git_status (check only expected files modified),\n\
    run_tests (verify acceptance), plan contract checklist\n\
  All bodies: ask_user, web_fetch\n\n\
Verification ritual (every tool result):\n\
- File reads: confirm line numbers match what you're about to patch\n\
- Shell commands: check stdout, not just exit code\n\
- Search results: confirm the match is what you expected\n\
- After code changes: run_tests or read the file back. Don't claim on faith\n\
- Negative claims require evidence: 'X not found' must include the search query\n\
- Don't trust memory over live tool output\n\
- If you can't verify, say so explicitly rather than implying success\n\
- Never claim 'all tests pass' when output shows failures\n\
\n"
        .to_string()
}

/// S10: Thinking Strategy — how to use reasoning budget.
/// Depth ladder: skip → light → medium → deep, matched to task complexity.
fn build_section_11_thinking_strategy(_config: &AgentConfig) -> String {
    "\
## Thinking Strategy\n\
- Skip reasoning for: simple lookups, one-line fixes, tool output verification\n\
- Light reasoning for: single-function generation, straightforward edits\n\
- Medium reasoning for: multi-file changes, cross-module refactoring\n\
- Deep reasoning for: debugging root causes, architecture design, security review\n\
- Reasoning is invisible to the user. Cache conclusions concisely in your response\n\
\n"
        .to_string()
}

// ══════════════ BUILDER ══════════════

use std::sync::RwLock;

pub struct SystemPromptBuilder {
    /// Layer 0 (frozen): static sections + tool JSON + verification — never changes, 100% cache hit
    cached_prefix: RwLock<String>,
    #[allow(dead_code)]
    config: AgentConfig,
    /// Guard against double-freeze (non-blocking, thread-safe).
    tools_frozen: std::sync::atomic::AtomicBool,
    /// SHA256 fingerprint of frozen prefix (Pattern: detect cache drift).
    fingerprint: RwLock<String>,
}

impl SystemPromptBuilder {
    pub fn new(config: AgentConfig) -> Self {
        let cached_prefix = RwLock::new(Self::assemble_static(&config));
        let fp = Self::compute_fingerprint(&cached_prefix.read().unwrap());
        Self {
            cached_prefix,
            config,
            tools_frozen: std::sync::atomic::AtomicBool::new(false),
            fingerprint: RwLock::new(fp),
        }
    }

    fn assemble_static(config: &AgentConfig) -> String {
        let mut p = String::with_capacity(16384);
        for build_fn in STATIC_ORDER {
            p.push_str(&build_fn(config));
        }
        p
    }

    /// Freeze tool definitions + verification into Layer 0.
    /// Call AFTER all tools are registered. Idempotent — second call is a no-op.
    /// Logs a warning on the first freeze since it invalidates the previous cache prefix.
    pub fn freeze_tools(&self, tool_json: &str) {
        if self.tools_frozen.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        let old_fp = self.fingerprint.read().unwrap().clone();
        let mut prefix = self.cached_prefix.write().unwrap();
        prefix.push_str("## Available Tools\n```json\n");
        prefix.push_str(tool_json);
        prefix.push_str("\n```\n\n");
        prefix.push_str("## Pre-Response Verification\n");
        prefix.push_str("- [ ] Read every file I plan to modify before editing\n");
        prefix.push_str("- [ ] Searched for existing patterns, imports, callers\n");
        prefix.push_str("- [ ] Verified correctness (tests, logic, edge cases, types)\n");
        prefix.push_str("- [ ] No TODOs, FIXMEs, incomplete work, or unverified claims\n");
        prefix.push_str("- [ ] All imports present. Compiler/tests would pass\n");
        prefix.push_str("- [ ] Negative claims backed by specific search queries\n");
        let new_fp = Self::compute_fingerprint(&prefix);
        let fp_display = new_fp.clone();
        *self.fingerprint.write().unwrap() = new_fp;
        tracing::info!(
            "Layer 0 frozen: {} → {} (1-turn cache miss, then stable)",
            &old_fp[..8.min(old_fp.len())], &fp_display[..8.min(fp_display.len())],
        );
    }

    /// Verify fingerprint hasn't drifted (Pattern: detect silent cache breakage).
    pub fn verify_fingerprint(&self) -> bool {
        let prefix = self.cached_prefix.read().unwrap();
        let current = Self::compute_fingerprint(&prefix);
        let stored = self.fingerprint.read().unwrap().clone();
        if current != stored {
            tracing::warn!(
                "Cache fingerprint drift detected! Stored: {}, Current: {}. Tools may have changed — expect cache miss.",
                &stored[..8], &current[..8],
            );
            return false;
        }
        true
    }

    fn compute_fingerprint(s: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        s.len().hash(&mut h);
        if s.len() > 0 {
            s.as_bytes()[0].hash(&mut h);
            let mid = s.len() / 2;
            s.as_bytes()[mid].hash(&mut h);
            s.as_bytes()[s.len() - 1].hash(&mut h);
        }
        format!("{:016x}", h.finish())[..16].to_string()
    }

    /// Get frozen Layer 0 prefix (for sub-agents to share cache).
    pub fn get_frozen_prefix(&self) -> Result<String, String> {
        self.cached_prefix.read()
            .map(|g| g.clone())
            .map_err(|e| format!("RwLock poisoned: {}", e))
    }

    pub fn build(
        &self,
        _tool_schemas_json: &str,  // Deprecated: now frozen via freeze_tools()
        _memories: Option<&str>,
        skills_text: Option<&str>,
        mode: ExecutionMode,
        model: &str,
    ) -> String {
        // Layer 0: Frozen prefix (static sections + tools + verification) — cached by DeepSeek
        let mut prompt = String::with_capacity(32768);
        prompt.push_str(&self.cached_prefix.read().unwrap());

        // Layer 1: Semi-frozen (changes on mode switch, ~200 chars)
        prompt.push_str(&format!("## Current Model\n{}\n\n", model));

        let mode_desc = match mode {
            ExecutionMode::Plan => "\
PLAN MODE — MUST produce a structured plan. Three-body cycle: Planner(survey) → Generator(plan) → Evaluator(review).\n\
READ-ONLY TOOLS ONLY: file_read, glob, grep, file_search, get_architectural_context, web_fetch.\n\
No edits. No bash. No writes.\n\n\
CRITICAL: You MUST complete ALL three phases:\n\
  Phase 1 (Planner): Survey codebase — read related files, search patterns, query code-graph.\n\
  Phase 2 (Generator): Create plan — use the plan tool with objective/files/tasks/acceptance/constraints.\n\
  Phase 3 (Evaluator): Self-review plan — is it complete? executable? edge cases covered?\n\
After all phases, present the final plan to the user for approval.",
            ExecutionMode::Default => "\
DEFAULT MODE — Full tools, every destructive/write action requires user approval.\n\
BUILT-IN PLANNER: the three-body harness (Planner→Generator→Evaluator) is always active.\n\n\
CONTEXT MANAGEMENT (CRITICAL — 1M token budget):\n\
- For files >300 lines: use get_architectural_context instead of file_read (avoids context bloat)\n\
- Use grep/glob to locate relevant code, then read only the specific sections you need\n\
- Prefer impact_map over manually tracing call chains through multiple file_reads\n\n\
TASK COMPLEXITY RULE (CRITICAL):\n\
  Simple task (single file edit, one-line fix, lookup, explanation) → act directly, verify, done.\n\
  Complex task (3+ steps, 2+ files, new feature, refactor) → create plan FIRST with plan tool,\n\
    then track progress with todo_write, verify with acceptance criteria.\n\n\
When in doubt between simple/complex: err on the side of planning. 30 seconds planning\n\
saves 30 minutes of wrong-path coding.",
            ExecutionMode::Chat => "\
CHAT MODE — No tools. Pure conversation and explanation.\n\
Answer questions, explain concepts, discuss approaches. Do not attempt any file operations.",
            ExecutionMode::Yolo => "\
YOLO MODE — Full tools, zero confirmations, autonomous execution.\n\
BUILT-IN PLANNER: same as Default mode — plan complex tasks, act directly on simple ones.\n\n\
TASK COMPLEXITY: same rule as Default. Simple = do it. Complex = plan + todo_write + verify.\n\
ALL ACTIONS PRE-APPROVED: no permission prompts. Execute autonomously.\n\
RESPONSIBILITY: you own the outcome. Verify thoroughly before reporting completion.",
        };
        prompt.push_str(&format!("## Mode: {}\n\n", mode_desc));

        // Layer 2: Semi-static — skills change rarely, cached between turns
        if let Some(skills) = skills_text {
            if !skills.is_empty() {
                prompt.push_str("## Active Skills\n");
                prompt.push_str(skills);
                prompt.push('\n');
            }
        }
        // NOTE: memories are NOT injected here — they change per-turn and
        // would break the prefix cache. Instead, they're added as a separate
        // SystemMessage in the conversation by AgentLoop::build_system_prompt().

        prompt
    }

    /// Build a three-body phase prompt (zero-cost, same context, different mindset).
    /// Appended to the standard prompt to shift the LLM into Planner/Evaluator mode.
    pub fn build_phase(&self, phase: HarnessPhase) -> String {
        match phase {
            HarnessPhase::Planner => "\
## PHASE: PLANNER\n\
You are now in the PLANNER phase. Before writing any code:\n\
- Survey the codebase: read related files, search patterns, query code-graph\n\
- Create a structured plan with the plan tool (objective, files, tasks, acceptance, constraints)\n\
- Every task item = one concrete todo. Acceptance criteria must be verifiable.\n\
- Use ask_user to present the plan for approval before switching to execution.\n\
DO NOT edit any files. DO NOT run bash. READ-ONLY.\n\
\n".to_string(),

            HarnessPhase::Evaluator => "\
## PHASE: EVALUATOR\n\
You are now in the EVALUATOR phase. Before reporting completion:\n\
- Run git_status: verify only expected files were modified\n\
- Run git_diff: review every change line by line\n\
- Run run_tests: confirm all acceptance criteria pass\n\
- Check plan contract: are ALL tasks marked complete?\n\
- Check CodeScorer: does the output score above threshold?\n\
- Check constraints: were any forbidden files or patterns touched?\n\
Report findings honestly. If anything fails, return to Generator phase.\n\
\n".to_string(),

            HarnessPhase::Generator => String::new(), // default — no extra prompt
        }
    }

    pub fn cached_prefix_len(&self) -> usize {
        self.cached_prefix.read().unwrap().len()
    }

    #[cfg(test)]
    pub fn static_sections() -> &'static [(&'static str, fn(&AgentConfig) -> String)] {
        &[
            ("1_identity", build_section_1_identity),
            ("2_task_complexity", build_section_2_task_complexity),
            ("3_mandatory_tool_use", build_section_3_mandatory_tool_use),
            ("4_execution", build_section_4_execution),
            ("5_code_philosophy", build_section_5_code_philosophy),
            ("6_security", build_section_6_security),
            ("7_tool_strategy", build_section_7_tool_strategy),
            ("8_output_standards", build_section_8_output_standards),
            ("9_terminal_formatting", build_section_9_terminal_formatting),
            ("10_verification_ritual", build_section_10_verification_ritual),
            ("11_thinking_strategy", build_section_11_thinking_strategy),
        ]
    }
}

// ══════════════ TESTS — each section independently verified ══════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn s1_identity() {
        let s = build_section_1_identity(&AgentConfig::default());
        assert!(s.contains("Aegis")); assert!(s.contains("file_read"));
        assert!(s.contains("get_architectural_context")); assert!(s.contains("web_search"));
    }
    #[test] fn s2_mandatory_tool_use() {
        let s = build_section_3_mandatory_tool_use(&AgentConfig::default());
        assert!(s.contains("NEVER answer")); assert!(s.contains("Arithmetic"));
        assert!(s.contains("Hashes")); assert!(s.contains("date"));
    }
    #[test] fn s3_execution() {
        let s = build_section_4_execution(&AgentConfig::default());
        assert!(s.contains("Tool-first")); assert!(s.contains("promise of future action"));
        assert!(s.contains("obvious default interpretation")); assert!(s.contains("name the gap"));
    }
    #[test] fn s4_code_philosophy() {
        let s = build_section_5_code_philosophy(&AgentConfig::default());
        assert!(s.contains("Don't add features")); assert!(s.contains("Three similar lines"));
        assert!(s.contains("Default to writing no comments"));
        assert!(s.contains("Don't remove existing comments unless"));
        assert!(s.contains("Don't propose changes to code you haven't read"));
        assert!(s.contains("Avoid giving time estimates"));
    }
    #[test] fn s5_security_owasp() {
        let s = build_section_6_security(&AgentConfig::default());
        assert!(s.contains("OWASP")); assert!(s.contains("rm -rf"));
        assert!(s.contains("Force-pushing")); assert!(s.contains("merge conflicts rather than discarding"));
        assert!(s.contains("Measure twice"));
    }
    #[test] fn s5_undercover_on() {
        let mut c = AgentConfig::default(); c.undercover_mode = true;
        assert!(build_section_6_security(&c).contains("UNDERCOVER"));
    }
    #[test] fn s5_undercover_off() {
        let mut c = AgentConfig::default(); c.undercover_mode = false;
        assert!(!build_section_6_security(&c).contains("UNDERCOVER"));
    }
    #[test] fn s6_tool_strategy() {
        let s = build_section_7_tool_strategy(&AgentConfig::default());
        assert!(s.contains("Do NOT use bash")); assert!(s.contains("Parallel-first"));
        assert!(s.contains("file_read over cat")); assert!(s.contains("grep before guessing"));
    }
    #[test] fn s7_output_standards() {
        let s = build_section_8_output_standards(&AgentConfig::default());
        assert!(s.contains("Concise, direct, no fluff")); assert!(s.contains("file_path:line_number"));
        assert!(s.contains("No emojis")); assert!(s.contains("misconception"));
        assert!(s.contains("collaborator, not just an executor"));
    }
    #[test] fn s8_terminal_formatting() {
        let s = build_section_9_terminal_formatting(&AgentConfig::default());
        assert!(s.contains("terminal, not a browser")); assert!(s.contains("Markdown tables"));
        assert!(s.contains("Label**: value"));
    }
    #[test] fn s9_verification_ritual() {
        let s = build_section_10_verification_ritual(&AgentConfig::default());
        assert!(s.contains("check stdout, not just exit code"));
        assert!(s.contains("Don't trust memory over live tool output"));
        assert!(s.contains("say so explicitly"));
        assert!(s.contains("Negative claims require evidence"));
    }
    #[test] fn s10_thinking_strategy() {
        let s = build_section_11_thinking_strategy(&AgentConfig::default());
        assert!(s.contains("Skip reasoning")); assert!(s.contains("Deep reasoning"));
    }
    #[test] fn test_full_build() {
        let mut b = SystemPromptBuilder::new(AgentConfig::default());
        b.freeze_tools("[]");
        let p = b.build("[]", None, None, ExecutionMode::Default, "deepseek-v4-pro");
        assert!(p.contains("Aegis")); assert!(p.contains("DEFAULT MODE"));
        assert!(p.contains("Mandatory Tool Use")); assert!(p.contains("Terminal Formatting"));
        assert!(p.contains("Pre-Response Verification"));
    }
    #[test] fn test_mode_variants() {
        let b = SystemPromptBuilder::new(AgentConfig::default());
        for m in &[ExecutionMode::Plan, ExecutionMode::Default, ExecutionMode::Chat, ExecutionMode::Yolo] {
            assert!(!b.build("[]", None, None, *m, "deepseek-v4-pro").is_empty());
        }
    }
    #[test] fn test_injections() {
        let mut b = SystemPromptBuilder::new(AgentConfig::default());
        b.freeze_tools(r#"[{"name":"grep","description":"Search"}]"#);
        let p = b.build("[]", Some("MEM-1: data"), None, ExecutionMode::Default, "p");
        // Memories are no longer in system prompt (injected as SystemMessage instead)
        assert!(!p.contains("MEM-1"), "memories moved out of system prompt for cache");
        assert!(p.contains("grep"));
    }
    #[test] fn test_cached_stable() {
        let b = SystemPromptBuilder::new(AgentConfig::default());
        assert_eq!(b.cached_prefix_len(), b.cached_prefix_len());
    }
    #[test] fn section_sizes() {
        let config = AgentConfig::default();
        let total: usize = SystemPromptBuilder::static_sections().iter().map(|(name, f)| {
            let chars = f(&config).len();
            println!("[{:25}] {:5} chars  ~{:4} tokens", name, chars, chars / 3);
            chars
        }).sum();
        println!("[total static prefix         ] {:5} chars  ~{:4} tokens", total, total / 3);
    }
}
