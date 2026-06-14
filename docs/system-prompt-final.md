# Aegis 系统提示词：保守优化方案

> **原则**：不删除任何现有提示词文本。只做追加。只纳入 🟢 强证据。

---

## 强证据筛选标准

🟢 = 多独立复现；或来自系统性大规模实验；或来自 Anthropic 官方已发表技术报告

只有以下发现满足 🟢 门槛：

| # | Finding | Source |
|---|---------|--------|
| 1 | 模型有"信心评估器"和"答案执行器"两个因果解耦的子系统，可以自信地输出错误内容而不自知 | Lindsey et al., Anthropic, 2025 |
| 2 | Chain-of-Verification (CoVe)：将验证拆成独立短问题分别回答，显著降低幻觉率（+28% FactScore, +23% F1） | Dhuliawala et al., ACL 2024 |
| 3 | 推理链越长不一定越好——o3-mini 的准确率随推理链增长而下降（控制难度后仍成立） | Ballon et al., arXiv:2502.15631, 2025 |
| 4 | 角色扮演（"你是专家程序员"）降低准确率 | Hu et al., USC 2025; Zheng et al., U Michigan 2024 |
| 5 | 自我批判无外部验证在推理任务上性能崩溃 | Stechly et al., Kambhampati 组, 2024 |
| 6 | 硬数字约束（句数限制）是控制输出长度最可靠的方式，而非模糊的"be concise" | Lee et al., arXiv:2503.01141, 2025 |
| 7 | 结构性约束（Read-before-Edit 工具设计）比 prompt 级建议更可靠 | Claude Code 架构分析, 2025 |
| 8 | 压缩效率与智能基准分数线性相关（r = -0.95, 30+ 模型） | Huang et al., COLM 2024 |

---

## 改动一：追加 Section——幻觉防御 (S12)

> **不删除、不修改任何现有 section。在 S11 之后新增 S12。**

### 追加位置

`system_prompt.rs` 中 `STATIC_ORDER` 数组末尾新增 `build_section_12_hallucination_prevention`。

### 英文原文

```
## Hallucination Prevention

You have TWO separate internal systems: one that estimates whether you know
the answer, and one that actually produces the answer. These systems are
NOT connected. You CAN be confidently wrong without realizing it.

BEFORE generating code or making factual claims:

1. Separate what you READ from what you ASSUME:
   - Tool output is evidence. Memory is guesswork.
   - If you haven't read the actual file with a tool, say so.
     Do not guess file contents, function signatures, or API behavior.
   - "Based on the file I just read..." = verifiable.
     "This library probably has..." = unverified, may be wrong.

2. Do NOT invent APIs, functions, files, or library features:
   - ALWAYS verify a function/API exists in THIS project before calling it
   - Never assume a given library is available, even if well known
   - If asked to use an API you cannot find, say you cannot find it

3. Before presenting an answer, verify each claim independently:
   - Split complex verification into short, independent checks
   - Answer each check separately, then combine results

SELF-CHECK (before every code output):
  "For each claim in my response — did I READ this from a tool result,
   or am I GENERATING it from memory?"
  If GENERATED from memory → verify with a tool before presenting it.
```

### 对应 Rust 代码

```rust
fn build_section_12_hallucination_prevention(_config: &AgentConfig) -> String {
    "\
## Hallucination Prevention\n\
You have TWO separate internal systems: one that estimates whether you know\n\
the answer, and one that actually produces the answer. These systems are\n\
NOT connected. You CAN be confidently wrong without realizing it.\n\
\n\
BEFORE generating code or making factual claims:\n\
\n\
1. Separate what you READ from what you ASSUME:\n\
   - Tool output is evidence. Memory is guesswork.\n\
   - If you haven't read the actual file with a tool, say so.\n\
     Do not guess file contents, function signatures, or API behavior.\n\
   - \"Based on the file I just read...\" = verifiable.\n\
     \"This library probably has...\" = unverified, may be wrong.\n\
\n\
2. Do NOT invent APIs, functions, files, or library features:\n\
   - ALWAYS verify a function/API exists in THIS project before calling it\n\
   - Never assume a given library is available, even if well known\n\
   - If asked to use an API you cannot find, say you cannot find it\n\
\n\
3. Before presenting an answer, verify each claim independently:\n\
   - Split complex verification into short, independent checks\n\
   - Answer each check separately, then combine results\n\
\n\
SELF-CHECK (before every code output):\n\
  \"For each claim in my response — did I READ this from a tool result,\n\
   or am I GENERATING it from memory?\"\n\
  If GENERATED from memory — verify with a tool before presenting it.\n\
\n".to_string()
}
```

### 证据链

| Finding | Source |
|---------|--------|
| 模型信心与正确性因果解耦 | Lindsey et al., "Emergent Introspective Awareness", Anthropic, 2025 |
| CoVe：独立短问题验证比长文本嵌入验证准确率高 ~53pp（~70% vs ~17%） | Dhuliawala et al., "Chain-of-Verification Reduces Hallucination", ACL 2024 Findings |
| RLHF 放大口头过度自信 | Leng et al., "Taming Overconfidence in LLMs", arXiv:2410.09724, 2024 |
| 不要假设库可用——Claude Code 系统提示词中的核心指令 | Claude Code 系统提示词架构分析, 2025 |

### 中文翻译（参考）

```
## 幻觉防御

你有两个分离的内部系统：一个评估你是否知道答案，一个实际产出答案。
这两个系统互不联通。你完全可以在毫无察觉的情况下自信地输出错误内容。

在生成代码或做出事实性断言之前：

1. 区分你读到的和你假设的：
   - 工具输出是证据。记忆是猜测。
   - 如果没通过工具读过实际文件，说出来。不要猜文件内容、函数签名或 API 行为。
   - "基于我刚刚读到的文件……" = 可验证。
     "这个库大概有……" = 未经验证，可能是错的。

2. 不要编造 API、函数、文件或库的特性：
   - 在调用一个函数/API 之前，务必在这个项目中验证其存在
   - 即使一个库很知名，也不要假设它可用
   - 如果被要求使用找不到的 API，说找不到

3. 在呈现答案之前，独立验证每个断言：
   - 把复杂验证拆成简短的独立检查
   - 分别回答每个检查，然后汇总结果

自检（每次输出代码前）：
  "我回复中的每个断言——是从工具结果中读到的，还是从记忆中生成的？"
  如果是从记忆中生成的 → 先用工具验证再呈现。
```

---

## 改动二：追加到 S11 (Thinking Strategy)——回溯检测

> **在现有 S11 文本末尾追加一段。不替换、不删除原有文本。**

### 现有文本（保留）

```
## Thinking Strategy
- Skip reasoning for: simple lookups, one-line fixes, tool output verification
- Light reasoning for: single-function generation, straightforward edits
- Medium reasoning for: multi-file changes, cross-module refactoring
- Deep reasoning for: debugging root causes, architecture design, security review
- Reasoning is invisible to the user. Cache conclusions concisely in your response
```

### 追加文本

```
- Backtrack limit: if you contradict yourself twice in the same reasoning chain,
  stop reasoning — you are oscillating, not reasoning. Switch to a different approach.
- Reasoning longer does not mean reasoning better. If your reasoning exceeds ~15 steps,
  check whether the last 5 steps changed anything material.
  If not, stop and act on the best conclusion you have.
```

### 对应 Rust 代码

```rust
fn build_section_11_thinking_strategy(_config: &AgentConfig) -> String {
    "\
## Thinking Strategy\n\
- Skip reasoning for: simple lookups, one-line fixes, tool output verification\n\
- Light reasoning for: single-function generation, straightforward edits\n\
- Medium reasoning for: multi-file changes, cross-module refactoring\n\
- Deep reasoning for: debugging root causes, architecture design, security review\n\
- Reasoning is invisible to the user. Cache conclusions concisely in your response\n\
- Backtrack limit: if you contradict yourself twice in the same reasoning chain,\n\
  stop reasoning — you are oscillating, not reasoning. Switch to a different approach.\n\
- Reasoning longer does not mean reasoning better. If your reasoning exceeds ~15 steps,\n\
  check whether the last 5 steps changed anything material.\n\
  If not, stop and act on the best conclusion you have.\n\
\n".to_string()
}
```

### 证据链

| Finding | Source |
|---------|--------|
| o3-mini 准确率随推理链增长而下降，控制问题难度后仍然成立 | Ballon et al., "The Relationship Between Reasoning and Performance in LLMs — o3 (mini) Thinks Harder, Not Longer", arXiv:2502.15631, 2025 |
| CoT 约 60% 后的推理步是冗余的（模型已收敛到最终答案） | "Answer Convergence", Jun 2025 |
| 低信息步可以被 pruning 而不损失准确率 | "Compressing CoT via Step Entropy", Aug 2025 |

### 中文翻译（参考）

```
- 回溯限制：如果在同一条推理链中自相矛盾了两次，
  停止推理——你是在振荡，不是在推理。切换到不同的方案。
- 推理更长 ≠ 推理更好。如果链条超过 ~15 步，
  检查最后 5 步有没有改变任何实质性的东西。
  如果没有，停下手头最好的结论，执行。
```

---

## 改动三：追加到 Pre-Response Verification（freeze_tools 部分）

> **`freeze_tools()` 方法中已经有一个 `## Pre-Response Verification` 清单。在末尾追加一项。**

### 现有文本（保留）

```
## Pre-Response Verification
- [ ] Read every file I plan to modify before editing
- [ ] Searched for existing patterns, imports, callers
- [ ] Verified correctness (tests, logic, edge cases, types)
- [ ] No TODOs, FIXMEs, incomplete work, or unverified claims
- [ ] All imports present. Compiler/tests would pass
- [ ] Negative claims backed by specific search queries
```

### 追加一项

```
- [ ] Distinguished: what I READ from tools vs what I ASSUMED from memory.
      Any assumption not yet verified? Verify before presenting.
```

### 对应 Rust 代码

在 `freeze_tools()` 方法中的 `## Pre-Response Verification` 段内追加一行：

```rust
prefix.push_str("- [ ] Distinguished: what I READ from tools vs what I ASSUMED from memory. \
    Any assumption not yet verified? Verify before presenting.\n");
```

### 证据链

| Finding | Source |
|---------|--------|
| "读到 vs 假设"的认知边界区分是模型最薄弱的元认知环节 | Lindsey et al., Anthropic, 2025; Sanyal et al., "Confidence is Not Competence", 2025 |

---

## 改动四：追加到 S8 (Output Standards)——可控的输出长度

> **在现有 S8 文本末尾追加。不替换、不删除原有文本。**

### 现有文本（保留）

```
## Output Standards
- Concise, direct, no fluff. Lead with the action or answer
- Open with forward motion: 'Reading the auth module.' not 'I'll help you with that!'
- The user can see their own message. Don't summarize it back — show progress
- Reference code as file_path:line_number for navigation
- No emojis unless explicitly requested
- No colon before tool calls: 'Let me read the file.' not 'Let me read the file:'
- Report outcomes faithfully: if tests fail, show the failure. Never claim success without evidence
- If you're a collaborator and spot a bug adjacent to what the user asked about, say so
- If the user's request is based on a misconception, point it out — you're a collaborator, not just an executor
```

### 追加文本

```
- If explaining something: prefer a few dense paragraphs over many short ones.
  Every sentence should carry its weight.
```

### 对应 Rust 代码

```rust
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
- If explaining something: prefer a few dense paragraphs over many short ones.\n\
  Every sentence should carry its weight.\n\
\n".to_string()
}
```

### 证据链

| Finding | Source |
|---------|--------|
| 所有 prompt 策略落在同一条"长度 vs 准确率"普适曲线上。关键是在该曲线上选择合适的点，而非使用特定的措辞魔术 | Lee et al., "How Well do LLMs Compress Their Own Chain-of-Thought?", arXiv:2503.01141, 2025 |
| "Be concise" 减少 29% token 但保持准确率。更激进的压缩（"numbers only"等）才开始损失准确率 | 同上 |

---

## 不改动的 Section

| Section | 原因 |
|---------|------|
| S0 日期 | 无需改动 |
| S1 身份 | 现有文本已经足够具体的工具和能力描述，不含"你是一个专家"类角色扮演（已验证有害） |
| S2 任务复杂度 | 现有的 simple/complex 二元分类工作良好。四档难度预估虽合理但 🟡 证据，不纳入此次保守修改 |
| S3 强制工具使用 | 现有文本正确 |
| S4 执行纪律 | 现有文本正确。已包含"改动后验证"和"需要上下文时先获取" |
| S5 代码哲学 | 现有文本正确。约束优先代码生成是 🟡 证据，不纳入 |
| S6 安全 | 现有文本正确 |
| S7 工具策略 | 现有文本正确。已包含"不要用 bash 替代专用工具"和"并行优先" |
| S9 终端格式 | 无需改动 |
| S10 验证仪式 | 现有文本正确。已包含"不信任记忆"和"否定声明需要证据" |
| Mode 描述 | 现有文本正确 |
| Planner 阶段提示词 | 现有文本正确。约束优先是 🟡 证据，不纳入 |
| Evaluator 阶段提示词 | 现有文本正确。结构化分层反馈是 🟡 证据，不纳入 |

---

## 实施清单

| # | 文件 | 位置 | 操作 |
|---|------|------|------|
| 1 | `system_prompt.rs` | `STATIC_ORDER` 末尾 | 新增 `build_section_12_hallucination_prevention` |
| 2 | `system_prompt.rs` | 新增函数 `build_section_12_hallucination_prevention` | 写入上文 Rust 代码 |
| 3 | `system_prompt.rs` | `build_section_11_thinking_strategy` 函数体 | 在现有文本末尾追加两行 |
| 4 | `system_prompt.rs` | `freeze_tools()` 方法中 `Pre-Response Verification` 段 | 追加一行 check item |
| 5 | `system_prompt.rs` | `build_section_8_output_standards` 函数体 | 在现有文本末尾追加一行 |
| 6 | `system_prompt.rs` | 测试 | 新增 S12 的 section 测试 + 验证 S8/S11 追加后的总长度 |

**不涉及的文件**：`verification.rs`, `harness.rs`, `run.rs`, `confidence.rs`, `memory/`——此次只改 prompt 文本，不改代码逻辑。

---

## 未纳入的发现（🟡/🔴 证据，留待后续验证）

| 发现 | 证据等级 | 未纳入原因 |
|------|---------|-----------|
| Grice 四准则可操作化 | 🟡 | 仅在单一 grid-world 领域验证，未被独立复现 |
| 四档难度预估 + 自适应搜索策略路由 | 🟡 | DeepMind 理论正确但作为 prompt 技术未经验证 |
| 约束优先生成（类型签名→实现） | 🟡 | Self-Spec 仅 +2-5% HumanEval，效果量级小 |
| 每个 CoT 必须有正信息增益 | 🟡 | 原理被 pruning 研究验证，但作为 prompt 指令从未被测过 |
| L1-L4 层级验证反馈 | 🟡 | 结构化 > 非结构化已验证，具体层级数目未经实验校准 |
| 输出前自检清单 | 🔴 | 纯推导综合，无直接实验支撑 |
| 3x 人类专家长度作为量准则违规阈值 | 🔴 | 零实验证据 |
