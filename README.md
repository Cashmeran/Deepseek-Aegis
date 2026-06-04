<p align="center">
  <img alt="Aegis" src="https://raw.githubusercontent.com/Cashmeran/deepseek-aegis/main/docs/aegis-banner.svg" width="540">
</p>

<p align="center">
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg?style=flat-square&color=8b949e&labelColor=161b22" /></a>
  <a href="https://github.com/Cashmeran/deepseek-aegis/actions"><img src="https://img.shields.io/github/actions/workflow/status/Cashmeran/deepseek-aegis/release.yml?style=flat-square&label=ci&labelColor=161b22&logo=githubactions&logoColor=white" /></a>
  <a href="https://github.com/Cashmeran/deepseek-aegis/stargazers"><img src="https://img.shields.io/github/stars/Cashmeran/deepseek-aegis.svg?style=flat-square&color=dbab09&labelColor=161b22&logo=github&logoColor=white" /></a>
</p>

<br/>

Aegis 是 DeepSeek V4 的终端编程代理。Rust 写的，不到 20MB。三体分离引擎（规划 → 生成 → 验证）共用同一个模型，不同阶段用不同系统提示词，零额外 API 成本。33 个内置工具，因果记忆，代码知识图谱，启动 <200ms。

```
$ aegis
> 加个 JWT 认证中间件，支持 token 刷新和黑名单
```

规划器先调研代码库，锁定验收标准。生成器写代码。验证器跑 `cargo check` 和 `cargo test`，没过就回去修，最多 8 轮。

> [!TIP]
> DeepSeek 的前缀缓存是字节级别的。aegis 的系统提示词分成 Layer 0（角色/规则/安全 — 基本不变）和 Layer 1（工具列表/项目结构 — 很少变），对 Layer 0 做了 SHA256 指纹锁死，保证缓存键永不失效。实测缓存命中率 >90%，长对话 token 成本降低约 90%。

---

## 和别的 agent 有什么不同

| | Aegis | Reasonix | Claude Code | Aider |
|---|---|---|---|---|
| 后端 | DeepSeek V4 | DeepSeek V3/V4 | Anthropic | 任意 |
| 语言 | Rust | TypeScript | TypeScript | Python |
| 启动 | <200ms | <1s | <2s | <500ms |
| 缓存 | 双层前缀锁定 | 缓存优先 | 不适用 | 偶发 |
| 验证 | cargo check + test + git diff | SEARCH/REPLACE 审阅 | — | lint-fix |
| 记忆 | 因果图 + 语义搜索 | file-based | — | — |
| 代码图谱 | tree-sitter × 5 | — | — | repo-map |

---

## 安装

### 预编译二进制

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/Cashmeran/deepseek-aegis/main/install.sh | bash
```

```powershell
# Windows PowerShell（管理员运行）
irm https://raw.githubusercontent.com/Cashmeran/deepseek-aegis/main/install.ps1 | iex
```

### Cargo

```bash
cargo install aegis-cli --locked
```

### 源码

```bash
git clone https://github.com/Cashmeran/deepseek-aegis.git
cd deepseek-aegis
cargo build --release
./target/release/aegis
```

### 配置 API Key

首次运行没有 key 会直接让你输，自动保存。或者手动写。

<details>
<summary><strong>配置方式和环境变量</strong></summary>

`~/.aegis/config.toml`：

```toml
api_key = "sk-..."
model = "deepseek-v4-pro"      # 或 deepseek-v4-flash
effort = "max"                  # off / high / max
acp_port = 9876                 # ACP 服务端口，0 禁用
```

环境变量会覆盖配置文件：

```bash
export DEEPSEEK_API_KEY="sk-..."
export DEEPSEEK_MODEL="deepseek-v4-flash"
```

[获取 DeepSeek API Key](https://platform.deepseek.com/api_keys)

</details>

---

## 使用

```bash
aegis                              # 启动交互式 TUI
aegis --model deepseek-v4-flash     # 指定模型
aegis --effort high                 # 指定推理强度 (off / high / max)
aegis config                        # 显示配置目录
```

### 快捷键

| 按键 | 行为 |
|------|------|
| `Enter` | 发送消息 |
| `Esc` | 取消当前操作 / 清空输入 / 退出 |
| `Ctrl+D` | 退出 |
| `PgUp` `PgDn` | 翻页（消息区） |
| `Ctrl+C` | 复制选中文本 |
| `/` | 触发斜杠命令 |
| `!` | 内联执行 bash |

### 斜杠命令

| 命令 | 效果 |
|------|------|
| `/model` | 切换模型（pro ↔ flash） |
| `/mode` | 切换执行模式：default → plan → yolo → chat |
| `/clear` | 清空对话 |
| `/compact` | 压缩上下文 |
| `/skill <名称>` | 加载 skill |

---

## 引擎

### 三体分离

Aegis 的核心创新：同一模型，不同阶段使用不同 system prompt，零额外 API 成本。

```
用户输入
  │
  ▼
┌──────────┐  只读工具：file_read, grep, glob, code-graph
│ 规划器   │  产出：任务理解 + SprintContract 验收标准
└────┬─────┘
     │
     ▼
┌──────────┐  写工具：file_edit, file_write, bash, todo_write
│ 生成器   │  产出：代码变更
└────┬─────┘
     │
     ▼
┌──────────┐  验证：cargo check, cargo test, git diff
│ 验证器   │  不通过 → 自救循环（最多 8 轮）→ 回到生成器
└────┬─────┘  通过 → 输出给用户
     │
     ▼
   用户
```

### SprintContract

编码开始前，规划器自动生成验收契约，包含：
- 任务目标
- 可测量的验收标准
- 预期输出特征
- 阻塞依赖追踪

生成器完成后，验证器逐条核验。不通过不输出。

### 信心评分

对 Chain-of-Thought 做 6 维结构分析，不依赖额外 LLM 调用：

| 维度 | 检测 |
|------|------|
| 幻觉标记 | 引用不存在的文件/函数 |
| 一致性 | 前后逻辑是否自洽 |
| 步骤内聚 | 每步是否有明确输入→输出 |
| 根因深度 | 是否触及根本原因而非表面修复 |
| 证据锚定 | 是否基于实际代码而非猜测 |
| 分支剪枝 | 是否排除了不可能的路径 |

---

## 上下文管理

### 分层系统提示

```
┌─────────────────────────────────┐
│ Layer 0: Frozen Prefix          │ ← SHA256 指纹锁定，缓存永不失效
│  角色、规则、安全约束           │
├─────────────────────────────────┤
│ Layer 1: Semi-Frozen            │ ← 仅在工具 schema 变化时失效
│  工具列表 JSON、项目结构        │
├─────────────────────────────────┤
│ Per-Turn                        │ ← 每轮重建
│  记忆检索、图谱上下文、对话历史 │
└─────────────────────────────────┘
```

### 6 级自适应折叠

| 级别 | 触发条件 | 策略 |
|------|---------|------|
| 1 | 75% 窗口 | 收缩工具结果（5%/条） |
| 2 | 80% 窗口 | 折叠最早 25% 消息 |
| 3 | 85% 窗口 | 折叠最早 50% 消息 |
| 4 | 90% 窗口 | 保留尾部 25%，其余归档 |
| 5 | 95% 窗口 | Force Summary（强制 LLM 总结） |
| 6 | 98% 窗口 | 退出并建议 `/clear` |

### 前缀缓存

DeepSeek 磁盘缓存自动命中重复前缀（角色定义、工具 schema、项目概述）。实测缓存命中率 >90%，长对话成本降低约 90%。

---

## 记忆系统

### GAAMA 因果图

```
Bug: NPE in auth.rs
    │
    ├─[root_cause]→ 未检查 token 是否为 None
    │
    └─[fix]→ commit a1b2c3d: 添加 token.is_some() 检查
         │
         └─[recurrence]→ 3 周后同样问题再次出现
              │
              └─[learned]→ 需要编译期保证（Option<T> 而非裸指针）
```

### CraniMem 门控

三条因素决定记忆是否被检索：

- **时间衰减**：越久远的记忆权重越低
- **访问频率**：经常命中的记忆得到强化
- **因果相关性**：当前问题与记忆的因果距离

### SYNAPSE 双路检索

- **BM25 路**：字符串匹配，保证召回（always available）
- **KNN 路**：embedding 向量语义搜索（需 `embedding` feature，ONNX runtime）

---

## 代码知识图谱

```
文件系统
  │ tree-sitter 解析（5 语言）
  ▼
┌─────────────────────────────────┐
│ SQLite + WAL                     │
│                                  │
│ nodes:  函数、类、接口、模块     │
│ edges:  calls, imports, extends  │
│                                  │
│ 查询：BFS 遍历、上下游、影响范围│
│ 增量：仅解析变更文件            │
└─────────────────────────────────┘
```

---

## 工具

33 个内置工具。

**文件** `file_read` `file_edit` `file_write` `apply_patch` —
edit 要求 old_string 在文件中精确出现一次，编辑前需要先读过文件（ReadTracker 追踪）。

**搜索** `grep` `glob` `file_search` `web_search` `web_fetch`

**代码** `bash`（独立进程沙箱，超时 120s） `run_tests` `git_status` `git_diff` `git_log` `lsp`

<details>
<summary><strong>全部工具列表</strong></summary>

| 类别 | 工具 |
|------|------|
| 文件 | `file_read` `file_edit` `file_write` `apply_patch` |
| 搜索 | `grep` `glob` `file_search` `web_search` `web_fetch` |
| 代码 | `bash` `run_tests` `git_status` `git_diff` `git_log` `lsp` |
| 规划 | `plan` `todo_write` `task_create` `task_list` `task_update` |
| 审查 | `review` `diagnostics` `validate` |
| 元工具 | `agent` `skill` |
| 基础设施 | `ask_user` `remember` `cron` `sleep_` `config` `worktree` `tool_search` |

Bash 危险命令（`rm -rf`、`git push --force`、`chmod 777` 等）默认拦截，需用户审批。

</details>

---

## 架构

```
crates/
├── core/         智能体循环、LLM 客户端、工具系统、类型定义        10,450 行
├── tools/        33 个工具实现                                     7,232 行
├── cli/          终端 UI、事件循环、应用层                         69,907 行
├── memory/       因果记忆（GAAMA + CraniMem + SYNAPSE）            1,545 行
├── code-graph/   Tree-sitter 解析 + SQLite 知识图谱                2,808 行
├── mcp/          MCP 协议 + ACP 服务端（HTTP/SSE）                  1,834 行
├── sandbox/      进程级安全隔离                                      631 行
└── desktop/      Tauri v2 桌面应用（独立，未加入 workspace）       2,180 行
```

**依赖注入设计**：core 不依赖 memory / code-graph / sandbox。全部通过 trait object + 闭包注入：

```rust
// 示例：core 不依赖 memory crate
agent.with_memory(Arc::new(move |query: &str| -> String {
    // 外部实现，core 不需要知道细节
    memory_store.search(query).unwrap_or_default()
}));
```

这使得 core 可以独立编译、独立测试、组件随意替换。

---

## 开发

```bash
cargo build --release
cargo test --workspace            # 1109 tests, 0 warnings
cargo check --workspace
cargo run --features perf -- --perf-log perf.log
```

---

## 不做

- **多供应商。** aegis 只支持 DeepSeek。前缀缓存和三体引擎的分离 prompt 设计依赖 DeepSeek 字节一致的缓存行为。换模型这两条都得重做。
- **IDE 插件。** 终端优先。TUI 是主界面。
- **Web 仪表盘。** token 用量、缓存命中率、实时成本都显示在 TUI 状态栏里。

---

<a href="https://www.star-history.com/?repos=loopfz%2Fdeepseek-aegis&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=Cashmeran/deepseek-aegis&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=Cashmeran/deepseek-aegis&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=Cashmeran/deepseek-aegis&type=date&legend=top-left" />
 </picture>
</a>

---

<p align="center">
  <sub>Apache 2.0 · <a href="./LICENSE">LICENSE</a> · <a href="https://github.com/Cashmeran/deepseek-aegis">GitHub</a></sub>
</p>
