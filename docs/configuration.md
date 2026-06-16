# 配置

## 配置文件

`~/.aegis/config.toml`（Windows: `%USERPROFILE%\.aegis\config.toml`）

```toml
api_key = "sk-..."
model = "deepseek-v4-pro"
```

## 环境变量

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_API_KEY` | API Key（优先于配置文件） |
| `DEEPSEEK_MODEL` | 覆盖默认模型 |

## 模型

| 模型 | 上下文 | 说明 |
|------|--------|------|
| `deepseek-v4-pro` | 1M tokens | 默认，推荐 |
| `deepseek-v4-flash` | 1M tokens | 快速模式 |

## 执行模式

桌面端底部状态栏切换：

| 模式 | 说明 |
|------|------|
| `default` | 标准模式 |
| `yolo` | 全自动执行 |
| `chat` | 纯对话 |

## MCP 配置

项目根目录创建 `.mcp.json`：

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

## 记忆系统

自动存储到 `.aegis/memory.db`，包含对话历史、Bug 修复经验、项目偏好。
