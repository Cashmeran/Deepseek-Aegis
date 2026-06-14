# 配置参考

## 配置文件位置

| 平台 | 路径 |
|------|------|
| Linux / macOS | `~/.aegis/config.toml` |
| Windows | `%USERPROFILE%\.aegis\config.toml` |

## 基本配置

```toml
# ~/.aegis/config.toml
api_key = "sk-..."
model = "deepseek-v4-pro"
effort = "max"
```

## 环境变量

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_API_KEY` | API Key（优先于配置文件） |
| `DEEPSEEK_MODEL` | 覆盖默认模型 |

## 模型选项

| 模型 | 上下文 | 最大输出 | 推理 |
|------|--------|---------|------|
| `deepseek-v4-pro` | 1M tokens | 384K tokens | 支持 |
| `deepseek-v4-flash` | 1M tokens | 8K tokens | 支持 |

## 推理强度

在 TUI 中按 `/model` 选择，或通过配置文件 `effort` 字段设置：

- `max` — 深度推理，适合复杂编程任务
- `high` — 中等推理
- `off` — 无推理（flash 模式）

## 执行模式

按 `Shift+Tab` 循环切换：

| 模式 | 说明 |
|------|------|
| `default` | 标准模式，高风险工具需确认 |
| `plan` | 只读 + 规划，不修改文件 |
| `yolo` | 全自动执行，跳过所有确认 |
| `chat` | 纯对话，不执行工具 |

## 沙箱配置

```toml
sandbox_backend = "process"       # "process" / "none"
sandbox_mode = "workspace-write"  # "full" / "workspace-write" / "read-only"
```

## MCP 配置

在项目根目录创建 `.mcp.json`：

```json
{
  "mcp_servers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    }
  }
}
```

## 上下文管理

上下文窗口 1M tokens，自动 6 级折叠。可通过 `/compact` 手动触发压缩，通过 `/context` 查看当前用量。

## 记忆系统

记忆存储在 `.agent/memory.db`（SQLite）。包含：

- 因果记忆图（Bug → 修复 → 复发模式）
- 偏好记忆
- 自动巩固（每 5 轮）
