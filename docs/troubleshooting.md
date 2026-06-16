# 常见问题

## API / 连接

### "DEEPSEEK_API_KEY not set"

1. 设置环境变量：`export DEEPSEEK_API_KEY="sk-..."`
2. 或在桌面端设置 → 通用 → 输入 API Key
3. 或创建 `~/.aegis/config.toml`：`api_key = "sk-..."`

### 网络错误 / 超时

- 检查是否能访问 `https://api.deepseek.com`
- 部分网络环境可能需要代理：`export HTTPS_PROXY=http://127.0.0.1:7890`

## 桌面端

### 白屏

- `cd crates/desktop && npm install`
- 清除构建缓存：`cargo clean && cargo build`

### Agent 找不到文件

- 确保在桌面端先打开了一个项目
- 文件操作会基于当前项目根目录
- 使用列表中的项目切换功能

### 代码图谱为空

- 代码图谱在打开项目时自动扫描
- 大型项目首次扫描需要几十秒
- 右键面板 → 图谱 tab 查看

## 工具执行

### bash 命令被拦截

危险命令自动拦截。切换到 Yolo 模式可绕过（不推荐）。

### 文件编辑失败

`file_edit` 要求 `old_string` 精确匹配。先用 `file_read` 确认文件内容。

## 会话管理

会话保存在 `.aegis/sessions/` 下，左侧栏可切换和删除。
