# Changelog

## [0.1.0] — 2026-06-04

Initial public release.

### Core Engine
- Three-body agent harness: Planner → Generator → Evaluator with same-model phase separation
- SprintContract: acceptance criteria bound before coding, verified after
- Pain6 self-rescue: up to 8 rounds of escalating verification and auto-correction
- Confidence scoring: chain-of-thought structural analysis (6 weighted dimensions)
- Tool call repair: 4-pass pipeline (Scavenge → Truncation → Storm → Flatten)
- Parallel tool execution with concurrency safety gating (ConcurrentSafe / ConcurrentUnsafe)

### Context & Memory
- 1M-token context window with 6-level adaptive folding
- DeepSeek prefix caching (~90% cost reduction on repeated prefixes)
- 11-section layered system prompt (Layer 0 frozen, Layer 1 semi-frozen)
- GAAMA causal memory graph: experience → error → fix relationships
- CraniMem gating: time decay × access frequency × causal relevance
- Code knowledge graph: tree-sitter (Rust/Python/TypeScript/JavaScript/Go) + SQLite + BFS traversal

### Tools (33 built-in)
- File ops: `file_read`, `file_edit`, `file_write`, `apply_patch` with read-before-edit tracking
- Search: `grep`, `glob`, `file_search`, `web_search`, `web_fetch`
- Code: `bash` (sandboxed), `run_tests`, `git_status`, `git_diff`, `git_log`, `lsp`
- Planning: `plan`, `todo_write`, `task_create`, `task_list`, `task_update`
- Agents: `agent` (sub-agent spawning), `skill`
- Review: `review`, `diagnostics`, `validate`
- Infrastructure: `ask_user`, `remember`, `cron`, `sleep`, `config`, `worktree`

### Terminal UI
- Ratatui-based TUI with syntax highlighting and markdown rendering
- Real-time streaming with thinking/content separation
- Diff coloring (+green/-red) for file edits
- Slash commands: `/compact`, `/clear`, `/resume`, `/goal`, `/export`
- Mouse support: scroll, text selection, copy to clipboard
- Session resume from saved conversations

### Desktop
- Tauri v2 desktop application with aegis dark theme
- React + Zustand frontend with real-time streaming display
- Session management, file tree, code graph visualization
- `get_architectural_context` tool for code dependency analysis
- Automatic code graph indexing on project open

### Platform
- Windows, Linux, macOS
- One-liner install via curl (Linux/macOS) or irm (Windows)
