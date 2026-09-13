# PX Agent GUI

> 最小、安全、原生的 Grok 与 Antigravity 官方 CLI 桌面客户端。

PX Agent GUI 基于 **Tauri 2 + 系统 Webview** 构建，采用直接进程管理（ACP / Stdio）模式驱动本机已安装的官方 CLI。

⚠️ **重要定位声明**：
**本客户端绝不替代官方 CLI**。登录认证、凭据保管、配额额度计算与文件落地修改等核心逻辑完全保留在官方 CLI 体系中。本仓不引入任何 API Key、Codex Router、OAuth Client、自定义沙箱或二次代理。

---

## 核心特性

1. **零密钥代理**：不存储任何 API Key 与 Token，完全复用本机官方 CLI 的登录凭据状态。
2. **官方状态探测与向导**：
   - 启动前自动探测 `grok` 与 `agy` 的安装与登录就绪状态。
   - 若未安装或未登录，界面仅提供官方标准安装与登录指引命令，点击即可一键复制。
3. **双官方后端支持（单一会话模型）**：
   - **Grok**：基于标准 ACP (Agent Client Protocol) JSON-RPC 协议与 `grok agent stdio` 交互。
   - **Antigravity (agy)**：基于官方 NDJSON 流水线与 `agy --input-format stream-json --output-format stream-json` 交互。
4. **流式输出实时回显**：实时解析并分离显示模型的思考过程（Thoughts）与正文回答（Text deltas）。
5. **安全权限卡点（默认拒绝放行）**：
   - 当 CLI 执行具有潜在风险的系统操作（终端命令、危险文件访问）时，客户端捕获权限审批请求并弹出卡片。
   - 默认选项为“拒绝”，只有用户主动点击“允许”才进行单次放行。

---

## 快速启动

### 方式一：开发调试模式

确保本机已安装 Node.js (pnpm) 与 Rust 工具链：

```bash
# 安装前端依赖
pnpm install

# 启动桌面端调试窗口
pnpm tauri dev
```

### 方式二：直接运行预构建 App

若已完成构建，可直接通过 macOS 命令启动窗口：

```bash
open src-tauri/target/debug/bundle/macos/px-agent-gui.app
```

或直接执行底层二进制：

```bash
./src-tauri/target/debug/px-agent-gui
```

---

## 实际使用的官方 CLI Help 开关与协议参数

### 1. Grok 后端

- **状态探测**：
  ```bash
  grok models
  ```
  - *说明*：退出码为 `0` 且输出 `You are logged in with grok.com.` 即判定已登录并获取可用模型列表；退出码非 0 即判定未登录。
- **启动参数与命令**：
  ```bash
  grok agent stdio
  ```
  - *对应 `grok agent --help`*：`stdio: Run the agent over stdio`。
  - *协议*：Agent Client Protocol (ACP) JSON-RPC。
  - *工作区*：在 `session/new` 请求中传入 `cwd: <workspace_path>`。
  - *权限开关*：关闭 `always-approve`，在检测到 `session/request_permission` 时拦截并弹窗，向 stdio 返回 `{"outcome": {"outcome": "selected", "optionId": "allow-once" | "reject-once"}}`。

### 2. Antigravity (agy) 后端

- **状态探测**：
  ```bash
  agy models
  ```
  - *说明*：退出码为 `0` 且输出模型列表即判定已就绪；退出码非 0 即判定未登录。
- **启动参数与命令**：
  ```bash
  agy --input-format stream-json --output-format stream-json
  ```
  - *对应 `agy --help`*：
    - `--input-format stream-json`：从 stdin 读取 NDJSON 格式的提示词（turn）。
    - `--output-format stream-json`：通过 stdout 输出机器可读的 NDJSON 事件流（`init`, `step_update`, `result`）。
  - *工作区*：子进程设置 `current_dir: <workspace_path>`。
  - *输入格式*：`{"event": "user", "message": {"content": "<prompt>"}}`。
  - *权限约束*：官方 headless 模式默认禁止未授权的高危操作（权限默认不放行）。

---

## 官方安装与登录参考

- **Grok**：
  - 官方安装脚本：`curl -fsSL https://x.ai/cli/install.sh | bash`
  - 登录指令：`grok login`
- **Antigravity**：
  - 官方安装脚本：`curl -fsSL https://antigravity.google/cli/install.sh | bash`
  - 登录指令：在终端直接运行 `agy` 交互完成登录
