<p align="center">
  <img alt="Aegis" src="https://raw.githubusercontent.com/Cashmeran/Deepseek-Aegis/main/docs/aegis-banner.svg" width="540">
</p>

<p align="center">
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg?style=flat-square&color=8b949e&labelColor=161b22" /></a>
  <a href="https://github.com/Cashmeran/Deepseek-Aegis/actions"><img src="https://img.shields.io/github/actions/workflow/status/Cashmeran/Deepseek-Aegis/release.yml?style=flat-square&label=ci&labelColor=161b22&logo=githubactions&logoColor=white" /></a>
  <a href="https://github.com/Cashmeran/Deepseek-Aegis/stargazers"><img src="https://img.shields.io/github/stars/Cashmeran/Deepseek-Aegis.svg?style=flat-square&color=dbab09&labelColor=161b22&logo=github&logoColor=white" /></a>
</p>

<br/>

Aegis 是终端编程代理。

```
$ aegis
> 加个 JWT 认证中间件，支持 token 刷新和黑名单
```


> [!TIP]
> DeepSeek 的前缀缓存是字节级别的。aegis 的系统提示词分成 Layer 0（角色/规则/安全 — 基本不变）和 Layer 1（工具列表/项目结构 — 很少变），对 Layer 0 做了 SHA256 指纹锁死，保证缓存键永不失效。实测缓存命中率 >90%，长对话 token 成本降低约 90%。

---


## 安装

### 方式一：一键安装（推荐）

**Windows** — 打开 PowerShell，粘贴运行：

```powershell
irm https://raw.githubusercontent.com/Cashmeran/Deepseek-Aegis/main/install.ps1 | iex
```

**Linux / macOS** — 打开终端，粘贴运行：

```bash
curl -fsSL https://raw.githubusercontent.com/Cashmeran/Deepseek-Aegis/main/install.sh | bash
```

这会自动下载、解压到 `~/.local/bin`（或 `%LOCALAPPDATA%\aegis`），并加入 PATH。之后在任何终端输入 `aegis` 即可启动。

### 方式二：手动下载

1. 打开 [Releases 页面](https://github.com/Cashmeran/Deepseek-Aegis/releases)
2. 下载对应系统的 zip / tar.gz
3. 解压，双击 `aegis`（Windows）或在终端 `./aegis`（Linux/macOS）

如果想让 `aegis` 在任何目录都能运行，把解压出来的 `aegis` 复制到 `/usr/local/bin/`（Linux/macOS）或手动加到系统 PATH（Windows）。

### 方式三：Cargo 安装

```bash
cargo install aegis-cli --locked
```

### 方式四：源码编译

```bash
git clone https://github.com/Cashmeran/Deepseek-Aegis.git
cd deepseek-aegis
cargo build --release
./target/release/aegis
```

### 配置 API Key

首次运行如果没配 Key，会在终端里提示你输入，自动保存到 `~/.aegis/config.toml`。

也可以提前手动创建（或以后修改）：

```toml
# ~/.aegis/config.toml
api_key = "sk-..."
model = "deepseek-v4-pro"
effort = "max"
```

环境变量也能用：`export DEEPSEEK_API_KEY="sk-..."`

[获取 DeepSeek API Key](https://platform.deepseek.com/api_keys)

---

## 使用

```bash
aegis                              # 启动交互式 TUI
aegis --model deepseek-v4-flash     # 指定模型（-m）
aegis --effort high                 # 推理强度 off / high / max（-e）
aegis chat                          # 等同于 aegis
aegis config                        # 显示配置目录
aegis --help                        # 完整命令行选项
```

### 快捷键

| 按键 | 行为 |
|------|------|
| `Enter` | 发送消息 |
| `Esc` | 取消 / 清空输入 / 退出 |
| `Ctrl+D` | 退出 |
| `Ctrl+C` | 中断当前任务 / 复制选中文本 |
| `PgUp` `PgDn` | 翻页 |
| `Shift+Tab` | 循环切换执行模式 |
| `!` | 内联执行 shell（如 `!cargo test`） |
| `@文件名` | 引用文件，输入时自动补全 |

### 斜杠命令

| 命令 | 效果 |
|------|------|
| `/clear` | 清空对话历史 |
| `/model` | 打开模型选择面板（pro / flash，推理强度） |
| `/mode` | 切换执行模式（default → plan → yolo → chat） |
| `/skill [名称]` | 列出或加载 skill |
| `/compact` | 手动压缩上下文 |
| `/thinking` | 开关 reasoning / thinking 模式 |
| `/verify` | 开关代码验证（cargo check + test） |
| `/snap` | 开关上下文快照 |
| `/sandbox` | 开关沙箱执行 |
| `/status` | 显示当前状态（模型、token、费用） |
| `/context` | 显示上下文窗口用量 |
| `/diff` | 显示 git diff |
| `/export` | 导出当前对话为 markdown |
| `/resume [会话]` | 恢复已保存的会话 |
| `/mcp` | MCP 服务器配置 |
| `/help` | 列出所有命令 |

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

## 社区

QQ 群：**654689667** — 反馈问题、讨论功能、交流经验。

---

## 开发

```bash
cargo build --release
cargo test --workspace            # 1109 tests, 0 warnings
cargo check --workspace
cargo run --features perf -- --perf-log perf.log
```


---

<a href="https://www.star-history.com/?repos=loopfz%2Fdeepseek-aegis&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=Cashmeran/Deepseek-Aegis&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=Cashmeran/Deepseek-Aegis&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=Cashmeran/Deepseek-Aegis&type=date&legend=top-left" />
 </picture>
</a>

---

<p align="center">
  <sub>Apache 2.0 · <a href="./LICENSE">LICENSE</a> · <a href="https://github.com/Cashmeran/Deepseek-Aegis">GitHub</a></sub>
</p>
