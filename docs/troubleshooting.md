# 常见问题

## API / 连接

### "DEEPSEEK_API_KEY not set"

Aegis 找不到 API Key。解决方法：

1. 设置环境变量：`export DEEPSEEK_API_KEY="sk-..."`
2. 或创建配置文件：`~/.aegis/config.toml`，写入 `api_key = "sk-..."`
3. 首次运行没有配置时，Aegis 会交互式提示输入

### 网络错误 / 超时

- 检查是否能访问 `https://api.deepseek.com`
- 部分网络环境可能需要代理：`export HTTPS_PROXY=http://127.0.0.1:7890`
- Aegis HTTP 客户端超时 600s，重试使用 jitter 退避

### 缓存命中率为 0%

前缀缓存在以下情况不生效：

- 系统提示词发生变化（工具列表更新、配置变更）
- 对话轮次太少（需要至少 2-3 轮积累前缀）
- 使用了不支持的模型

缓存命中率在状态栏显示，也可通过 `/stats` 查看。

## 终端 / 显示

### TUI 显示异常

- 确保终端支持 true color（Windows Terminal、iTerm2、Alacritty 等）
- 不要在普通 CMD 或传统终端中运行
- 如果出现残影，按 `Ctrl+L` 或在终端输入 `reset`

### 中文显示乱码

- Windows Terminal 设置为 UTF-8 编码
- macOS/Linux 确保 locale 为 UTF-8：`locale` 检查 `LC_ALL`

### 粘贴大段文本卡顿

Aegis 对大段粘贴（>200 字符，>5 行）自动使用引用标记而非直接插入，避免 TUI 渲染卡顿。发送时自动展开。

## 工具执行

### bash 命令被拦截

危险命令自动拦截，包括：`rm -rf /`, `git push --force`, `chmod 777`。切换到 Yolo 模式可绕过（不推荐）。

### 文件编辑失败

`file_edit` 要求 `old_string` 在文件中精确出现一次。如果：
- 存在多个匹配 → 缩小匹配范围
- 不存在匹配 → 先用 `file_read` 确认文件内容

### 子代理无响应

子代理有独立的轮次限制（默认 100 轮）和工具白名单。如果子代理卡住，按 `Esc` 取消当前轮。

## 会话管理

### 恢复之前的会话

```
/resume
```
会列出已保存的会话。也可以直接指定：
```
/resume session-003
```

### 会话文件在哪

`./.agent/sessions/session-XXX.json`（项目目录下）。每个会话包含完整的对话历史和元数据。

### 导出的文件乱码

`/export` 导出为 UTF-8 编码的 Markdown。如果编辑器显示乱码，请用 UTF-8 重新打开。

## 桌面端

### 桌面端白屏

- 确保 `cd crates/desktop && npm install` 完成
- 清除构建缓存：`npm run tauri:build -- --clean`
- 检查 Node.js 版本 >= 18

### 桌面端与 CLI 的关系

桌面端是独立构建，不参与 workspace。功能上和 CLI 共享 core/tools/memory 等 crate，但 UI 层完全独立。
