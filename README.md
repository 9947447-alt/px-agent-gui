# PX Agent GUI

> 选工作区、打开本机已装桌面端；也可薄封装官方 CLI。不替代 KayG / 官方 Antigravity。

日常用法：

- **Grok（审计）**：KayG / Grok Build GUI
- **Gemini（实现）**：官方 Antigravity 桌面

本仓探测并 `open` 上述已装应用（能传工作区就传，不能传就只打开 App）。未装则只显示安装提示，不崩溃。不引入 API Key、Codex Router、OAuth Client，也不把 KayG / Antigravity 源码拷进本仓。

可选：继续用本机 `grok` / `agy` CLI 开一轮会话（一次一后端）。**权限默认不放行。** Grok ACP 仅 `allow-once` / `reject-once`。agy headless 对高危命令自行拒绝，本窗口只展示「CLI 已拒绝」，没有「允许执行」，也无法从本客户端放行。

---

## 核心特性

1. **启动器**：选择工作区，探测 KayG / Grok Build GUI 与官方 Antigravity，存在才显示打开按钮。
2. **零密钥代理**：不存储任何 API Key 与 Token，完全复用本机官方 CLI / 官方桌面端的登录状态。
3. **官方 CLI 状态探测与向导**：
   - 启动前自动探测 `grok` 与 `agy` 的安装与登录就绪状态。
   - 若未安装或未登录，界面仅提供官方标准安装与登录指引命令，点击即可一键复制。
4. **双官方后端支持（单一会话模型）**：
   - **Grok**：基于标准 ACP (Agent Client Protocol) JSON-RPC 协议与 `grok agent stdio` 交互。
   - **Antigravity (agy)**：基于官方 NDJSON 流水线与 `agy --input-format stream-json --output-format stream-json` 交互。
5. **流式输出实时回显**：实时解析并分离显示模型的思考过程（Thoughts）与正文回答（Text deltas）。
6. **安全权限卡点（默认拒绝放行）**：
   - **Grok**：捕获 `session/request_permission`，仅单次允许或拒绝。
   - **agy**：卡片只展示 CLI 已拒绝；点关闭即可。本窗口不会假装放行。

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
  - *权限约束*：官方 headless 模式默认禁止未授权的高危操作。stdin 只接受 `event: user`，本窗口不发明权限放行事件，也不加 `--dangerously-skip-permissions`。

### 3. 桌面端探测与打开

- 用 macOS `mdfind`（bundle id）加上 `/Applications`、`$HOME/Applications` 等常见位置探测，不写死用户主目录。
- **KayG / Grok Build GUI**（审计）：`KayG.app` / `Grok GUI.app`（`ai.grok.build.gui`）/ `Grok Build GUI.app` / `Grok Build Desktop.app`。
- **Antigravity**（实现）：`Antigravity.app`（`com.google.antigravity`）。
- 打开：`open -a <已探测到的.app> [工作区]`。工作区不是目录则只打开 App。

---

## 官方安装与登录参考

- **Grok CLI**：
  - 官方安装脚本：`curl -fsSL https://x.ai/cli/install.sh | bash`
  - 登录指令：`grok login`
- **Antigravity CLI**：
  - 官方安装脚本：`curl -fsSL https://antigravity.google/cli/install.sh | bash`
  - 登录指令：在终端直接运行 `agy` 交互完成登录
- **桌面端**：从各自官方渠道安装 KayG / Grok Build GUI 与官方 Antigravity；本仓不提供安装包。
