# Aegis Desktop

Aegis 的桌面客户端，基于 Tauri v2 + React 构建。

## 概述

- 本地优先的桌面编程代理环境，工具执行全程可见
- 基于会话的工作流，绑定工作目录
- 流式输出，Markdown 实时渲染
- DeepSeek 后端，支持 reasoning_content 的 SSE 流式传输
- 显式权限审批，危险操作需用户确认

## 功能

- 桌面原生 UI，低开销
- Token 流式传输 + 推理过程实时展示
- 工具调用卡片（展开/折叠、耗时、状态）
- 多执行模式：Chat / Plan / Default / Yolo
- 会话持久化，支持 `/resume` 恢复
- 深色主题，Aegis 设计语言

## 快速开始（源码构建）

### 环境要求

- Node.js 18+ 或 Bun
- Rust 工具链 (cargo)
- Tauri CLI (`cargo install tauri-cli`)

### 安装运行

```bash
cd crates/desktop

npm install
npm run tauri:dev
```

### 打包

```bash
npm run tauri:build
```

## 技术栈

| 层 | 技术 |
| --- | --- |
| Desktop | Tauri v2 |
| UI | React 19 + Tailwind CSS |
| State | Zustand |
| Backend | Rust (Tauri commands + aegis-core) |
| AI | DeepSeek API (anthropic-compatible endpoint) |
| Build | Vite + Cargo |

## 安全模型

- 工具调用在 UI 中可见
- 审批模式：auto-run 或 ask first
- 破坏性操作（rm -rf, git push --force 等）默认拦截

## License

Apache 2.0 — 与主项目一致。
