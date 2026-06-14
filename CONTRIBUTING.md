# Contributing to Aegis

感谢你对 Aegis 的关注。欢迎任何形式的贡献。

## 准备工作

```bash
git clone https://github.com/Cashmeran/Deepseek-Aegis.git
cd deepseek-aegis
cargo build
cargo test --workspace
```

运行前确保设置了 `DEEPSEEK_API_KEY` 环境变量。

## 开发流程

1. Fork 仓库，从 `main` 分支创建 feature 分支
2. 修改代码，保持与现有代码风格一致
3. 确保 `cargo test --workspace` 全部通过
4. 确保 `cargo clippy --workspace -- -D warnings` 无告警
5. 提交 PR，描述清楚改了什么、为什么改

## 代码风格

- `cargo fmt` 格式化
- 偏好 `&str` 作为参数类型，`String` 仅在需要所有权时使用
- 公开类型实现 `Debug`
- 函数保持简洁（<50 行）
- 不引入不必要的依赖

## 项目结构

```
crates/
├── core/        智能体循环、LLM 客户端、工具系统、类型定义
├── tools/       33 个内置工具实现
├── cli/         终端 UI、事件循环、应用层
├── memory/      因果记忆系统
├── code-graph/  Tree-sitter 代码知识图谱
├── mcp/         MCP 协议 + ACP 服务端
├── sandbox/     进程级安全隔离
└── desktop/     Tauri 桌面应用（独立构建）
```

## 沟通

- 在开 Issue 或 PR 前先搜索已有内容，避免重复
- Bug 报告需包含：环境信息、复现步骤、期望行为、实际行为
- Feature 请求需说明：使用场景、期望效果
- QQ 群：654689667

## License

提交代码即表示同意以 Apache 2.0 协议授权你的贡献。
