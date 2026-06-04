# Claude Code 源码架构深度拆解

> 来源：`C:\Users\15231\claude\cc源码` | 1880 TypeScript/TSX 文件，~50万行
> 分析日期：2026-05-31
> 目的：为 aegis Python 交互原型提供架构参考

---

## 一、顶层模块划分（38个目录）

```
cc源码/
├── components/  389 文件  ← ★ TUI渲染层（Ink/React）
├── utils/       563 文件  ← ★ 工具函数（最大模块）
├── commands/    189 文件  ← 斜杠命令（/add-dir, /agents, /clear...）
├── tools/       184 文件  ← ★ 工具实现（Bash, Read, Write, Agent...）
├── services/    130 文件  ← 后端服务（API, MCP, 权限...）
├── hooks/       104 文件  ← React Hooks（状态/副作用管理）
├── ink/          96 文件  ← Ink 框架封装（终端渲染引擎）
├── bridge/       31 文件  ← Agent SDK 桥接层
├── constants/    21 文件  ← 常量（figures, prompts, limits...）
├── cli/          19 文件  ← CLI 入口 + 传输层
├── state/         6 文件  ← ★ 状态管理（Zustand 模式）
├── types/         7 文件  ← 类型定义
├── skills/       20 文件  ← 技能系统
├── tasks/        12 文件  ← 任务管理
├── keybindings/  14 文件  ← 键盘绑定
├── context/       9 文件  ← React Context
├── entrypoints/   8 文件  ← 启动入口
├── migrations/   11 文件  ← 数据迁移
├── plugins/       2 文件  ← 插件系统
├── buddy/         6 文件  ← Buddy 通知
├── screens/       3 文件  ← 全屏界面
├── vim/           5 文件  ← Vim 模式
│
├── Tool.ts              ← ★ 工具系统核心类型
├── Task.ts              ← ★ 任务生命周期
├── QueryEngine.ts       ← ★ Agent 主循环
├── commands.ts          ← 命令注册表
├── cost-tracker.ts      ← 成本追踪
└── costHook.ts          ← 成本钩子
```

## 二、五层架构

```
┌─────────────────────────────────────────────────┐
│ Layer 5: CLI Entry                                │
│ cli/print.ts, cli/structuredIO.ts                 │
│ → 终端 I/O，JSON 流输出                           │
├─────────────────────────────────────────────────┤
│ Layer 4: TUI Components (Ink/React)               │
│ components/ 389 files                             │
│ → Messages, PromptInput, StatusLine, App          │
├─────────────────────────────────────────────────┤
│ Layer 3: State + Hooks                            │
│ state/AppStateStore.ts, hooks/ 104 files          │
│ → Zustand 模式 store，事件驱动状态变更             │
├─────────────────────────────────────────────────┤
│ Layer 2: Agent Loop + Services                    │
│ QueryEngine.ts, services/, tools/184 files        │
│ → LLM 调用，工具执行，权限检查                     │
├─────────────────────────────────────────────────┤
│ Layer 1: Bridge + Transport                        │
│ bridge/31 files, cli/transports/                   │
│ → Agent SDK 通信，SSE/WebSocket                   │
└─────────────────────────────────────────────────┘
```

## 三、核心文件逐行拆解

### 3.1 `Tool.ts` — 工具系统（根文件）

```typescript
// 工具输入 Schema
type ToolInputJSONSchema = { type: 'object', properties?: {...} }

// 工具接口定义
interface Tool {
  name: string
  description: string
  inputSchema: ToolInputJSONSchema
  // 执行方法
  call(params, context): Promise<ToolResult>
  // 权限检查
  permissionCheck?: (params) => PermissionResult
}

// 工具匹配
function toolMatchesName(tool: Tool, name: string): boolean
```

**设计要点：**
- 每个工具是独立对象，包含 schema + 执行逻辑
- `ToolPermissionContext` 集中管理权限状态
- 工具执行返回 `ToolResultBlockParam`（Anthropic SDK 类型）

### 3.2 `Task.ts` — 任务生命周期

```typescript
type TaskType = 'local_bash' | 'local_agent' | 'remote_agent' 
              | 'in_process_teammate' | 'local_workflow' | 'monitor_mcp'

type TaskStatus = 'pending' | 'running' | 'completed' | 'failed' | 'killed'

type TaskStateBase = {
  id: string
  type: TaskType
  status: TaskStatus
  description: string
  abortController: AbortController  // 取消机制
}

type TaskContext = {
  getAppState: () => AppState    // 读取全局状态
  setAppState: (f) => void       // 更新全局状态（函数式更新）
}
```

**设计要点：**
- 所有异步操作通过 `AbortController` 统一取消
- `setAppState` 使用函数式更新（类似 React setState）
- 任务有明确的生命周期状态机

### 3.3 `QueryEngine.ts` — Agent 主循环

```typescript
// 核心流程：
// 1. 构建消息上下文（system prompt + history + user input）
// 2. 调用 LLM API（通过 services/api/claude.ts）
// 3. 处理响应：文本 → 流式输出，工具调用 → 执行工具 → 继续循环
// 4. 追踪用量和成本

// 关键函数：
async function query(
  messages: Message[],
  tools: Tools,
  context: ToolUseContext,
  options: QueryOptions
): Promise<Message[]>
```

**设计要点：**
- `ToolUseContext` 包含权限回调、中断信号、状态读写
- `SYNTHETIC_OUTPUT_TOOL` — 推测性输出（Speculation），CC 核心创新
- `getSlashCommandToolSkills()` — 斜杠命令作为特殊工具

### 3.4 `state/AppStateStore.ts` — 状态管理

```typescript
// Store 模式（类似 Zustand）
type Store<T> = {
  getState: () => T
  setState: (partial: Partial<T>) => void
}

// AppState 核心字段
type AppState = {
  messages: Message[]                    // 消息历史
  toolPermissionContext: {...}           // 工具权限
  speculationState: SpeculationState     // 推测状态
  todos: TodoList                        // 待办列表
  settings: SettingsJson                 // 用户设置
  // ... 大量其他字段
}

// 推测状态（Speculation）— CC 核心创新
type SpeculationState =
  | { status: 'idle' }
  | { 
      status: 'active'
      id: string
      abort: () => void
      messagesRef: { current: Message[] }  // 可变引用，避免数组复制
      writtenPathsRef: { current: Set<string> }
      boundary: CompletionBoundary | null
    }
```

**设计要点：**
- `createStore(initialState, onChangeAppState)` — 创建 store 并监听变化
- 使用 `MutableRef` 避免每帧复制大数组
- `CompletionBoundary` 标记任务完成点

### 3.5 `components/` — TUI 渲染层（389文件）

**关键组件分类：**

| 类别 | 文件 | 职责 |
|------|------|------|
| **消息** | Messages.tsx, Message.tsx, MessageResponse.tsx, MessageRow.tsx | 消息列表渲染 |
| **输入** | PromptInput/ (目录) | 输入框 + 自动补全 |
| **状态** | StatusLine.tsx | 底部状态栏 |
| **工具** | AgentProgressLine.tsx | 工具执行进度 |
| **布局** | App.tsx | 顶层包装 |
| **模态** | 各种 Dialog | 权限/确认弹窗 |

**MessageResponse.tsx 核心渲染逻辑：**
```tsx
// 每条 Assistant 消息包裹在 <MessageResponse> 中
// 渲染为：<NoSelect dimColor>  ⎿  &nbsp;</NoSelect>
//         <Box flexGrow={1}>{children}</Box>
// 嵌套检测：避免重复渲染 ⎿ 字符
```

**StatusLine.tsx 核心逻辑：**
```tsx
// 底部状态栏显示：
// 左：权限模式 + 上下文使用率
// 右：模型名 + 成本 + 速度
// 条件渲染：settings?.statusLine !== undefined
```

### 3.6 `constants/figures.ts` — 视觉常量

```typescript
// 平台适配
BLACK_CIRCLE = (macOS) ? '⏺' : '●'
BULLET_OPERATOR = '∙'
EFFORT_LOW = '○'    // ○
EFFORT_MEDIUM = '◐'  // ◐
EFFORT_HIGH = '●'    // ●
EFFORT_MAX = '◉'     // ◉
BLOCKQUOTE_BAR = '▎' // 引用条
BRIDGE_SPINNER_FRAMES = ['·|·', '·/·', '·—·', '·\\·']
BRIDGE_READY_INDICATOR = '·✓·'
```

### 3.7 `bridge/` — 桥接层（31文件）

**核心文件：**
- `bridgeMain.ts` — 桥接主入口
- `replBridge.ts` / `replBridgeHandle.ts` — REPL 桥接
- `inboundMessages.ts` — 入站消息处理
- `bridgePermissionCallbacks.ts` — 权限回调

**通信模式：**
```
TUI ←→ Bridge ←→ Agent SDK (Node.js) ←→ Anthropic API
         ↑
    SSE/WebSocket transports
```

## 四、数据流

```
用户输入 (PromptInput)
    │
    ▼
斜杠命令？→ commands/ 处理 → 直接返回
    │
    ▼
普通消息？→ 追加到 AppState.messages[]
    │
    ▼
QueryEngine.query(messages, tools, context)
    │
    ├→ 构建 System Prompt（constants/prompts.ts）
    ├→ 调用 LLM API（services/api/claude.ts）
    ├→ 流式返回文本块 → 追加到消息流
    ├→ 遇到工具调用 → Tool.call() → 结果追加到消息流 → 继续循环
    └→ 完成 → CompletionBoundary → 更新成本/用量
    │
    ▼
AppState 更新 → React 重渲染 → Ink 输出到终端
```

## 五、对 Python 原型的启示

可以直接复用的架构决策：

1. **消息模型**：Message = UserMessage | AssistantMessage | SystemMessage | ProgressMessage
2. **状态管理**：使用简单的 Store 模式（getState/setState），不需要 Redux
3. **工具系统**：每个工具是独立对象 `{name, schema, call()}`
4. **任务取消**：使用 `asyncio.Task` + `CancelledError` 替代 `AbortController`
5. **事件流**：Python `asyncio.Queue` 替代 Node.js EventEmitter
6. **渲染**：Textual 的 `reactive` 属性天然支持响应式更新

不需要的（对我们多余）：
- Bridge 层（我们有直接的后端连接）
- MCP 系统（我们用自己的工具注册）
- Buddy/通知系统
- 推测执行（第一版不需要）
- 插件系统

## 六、文件关联图

```
                    Tool.ts ─────────────────────────┐
                    (工具接口 + 权限类型)              │
                                                     │
         ┌───────────────────────────────────────────┤
         │                                           │
    QueryEngine.ts ────→ services/api/claude.ts      │
    (Agent主循环)         (LLM API调用)               │
         │                                           │
         ├──→ state/AppStateStore.ts                 │
         │    (消息历史 + 状态管理)                    │
         │         │                                 │
         │         └──→ components/Messages.tsx       │
         │              components/MessageResponse.tsx│
         │              components/StatusLine.tsx     │
         │              components/PromptInput/       │
         │                                           │
         └──→ tools/ (184文件) ──────────────────────┘
              (BashTool, ReadTool, WriteTool, AgentTool...)
              
    bridge/ ──→ Agent SDK (外部 Node.js 进程)
    (我们不需要，用 AgentLoop 替代)
```

---

**总结：CC 源码是 1880 文件、50万行的 TypeScript 项目。核心是 4 层：TUI(Ink) → State(Zustand) → Agent(QueryEngine) → Bridge(SSE)。Python 原型只需复刻前 3 层，第 4 层用我们的 AgentLoop。**
