# Security Policy

## 报告安全漏洞

如果你发现了安全漏洞，请**不要**开公开 Issue。

请发送邮件到项目维护者，或通过 GitHub 的 [private vulnerability reporting](https://github.com/Cashmeran/Deepseek-Aegis/security/advisories/new) 功能提交。我们会在 48 小时内回复。

## Aegis 安全架构

### 沙箱执行

Aegis 通过进程级隔离执行 bash 命令，限制文件系统和网络访问：

- 环境变量白名单
- 工作目录限定
- 命令超时 (120s)
- 危险命令拦截（`rm -rf /`, `git push --force`, `chmod 777` 等）

### API Key 安全

- API Key 存储在 `~/.aegis/config.toml`，文件权限 600
- 支持环境变量 `DEEPSEEK_API_KEY`（推荐用于 CI/CD）
- 日志中自动脱敏

### 工具权限

- 4 级执行模式：Default / Auto / Bypass / Yolo
- 高风险工具（bash、file_write、file_edit）默认需用户确认
- 子代理继承父代理的权限级别

## 支持的版本

当前仅维护 `main` 分支的最新版本。

## 已发现但未修复的漏洞

目前没有已知但未修复的安全漏洞。
