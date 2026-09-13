use crate::detector::find_binary;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamUpdatePayload {
    pub session_id: String,
    pub kind: String, // "text_delta" | "thought_delta" | "tool_call" | "error"
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOptionPayload {
    pub option_id: String,
    pub name: String,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestPayload {
    pub id: Value,
    pub session_id: String,
    pub tool_name: String,
    pub title: String,
    pub command: Option<String>,
    pub options: Vec<PermissionOptionPayload>,
    /// agy headless already denied; GUI must not offer or report a successful allow.
    #[serde(default)]
    pub already_denied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEndPayload {
    pub session_id: String,
    pub status: String,
    pub full_response: Option<String>,
}

pub struct ActiveSession {
    pub backend: String,
    pub session_uuid: String,
    pub child: Child,
    pub stdin_tx: tokio::sync::mpsc::Sender<String>,
}

pub struct SessionState {
    pub active: Arc<Mutex<Option<ActiveSession>>>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(None)),
        }
    }
}

pub async fn stop_active_session(state: &SessionState) {
    let mut lock = state.active.lock().await;
    if let Some(mut session) = lock.take() {
        let _ = session.child.kill().await;
    }
}

fn is_session_wide_allow(option_id: &str) -> bool {
    let id = option_id.to_ascii_lowercase();
    id.contains("always") || (id.contains("allow") && id.contains("session"))
}

fn is_once_option(option_id: &str) -> bool {
    option_id == "allow-once" || option_id == "reject-once"
}

/// Grok ACP may list session/always options; emit only allow-once / reject-once.
fn grok_permission_options_for_emit(params: &Value) -> Vec<PermissionOptionPayload> {
    let mut options = vec![];
    if let Some(opts) = params.get("options").and_then(|o| o.as_array()) {
        for opt in opts {
            if let Some(opt_id) = opt.get("optionId").and_then(|s| s.as_str()) {
                if !is_once_option(opt_id) {
                    continue;
                }
                let name = opt
                    .get("name")
                    .and_then(|s| s.as_str())
                    .unwrap_or(opt_id)
                    .to_string();
                let kind = opt
                    .get("kind")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                options.push(PermissionOptionPayload {
                    option_id: opt_id.to_string(),
                    name,
                    kind,
                });
            }
        }
    }
    options
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "未设置 HOME / USERPROFILE".to_string())
}

fn spawn_stderr_drain(stderr: Option<tokio::process::ChildStderr>) {
    let Some(stderr) = stderr else {
        return;
    };
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(_)) = reader.next_line().await {}
    });
}

/// agy headless already denied the tool. Cards are informational: close only.
fn agy_denied_options() -> Vec<PermissionOptionPayload> {
    vec![PermissionOptionPayload {
        option_id: "dismiss".to_string(),
        name: "关闭".to_string(),
        kind: Some("dismiss".to_string()),
    }]
}

#[derive(Debug, PartialEq, Eq)]
enum PermissionPlan {
    SendToGrokStdin,
    AcknowledgeAgyDenied,
}

fn plan_permission_response(backend: &str, option_id: &str) -> Result<PermissionPlan, String> {
    if is_session_wide_allow(option_id) {
        return Err(format!(
            "拒绝会话级放行 optionId={}，仅允许单次 allow-once / reject-once",
            option_id
        ));
    }
    match backend {
        "grok" => {
            if !is_once_option(option_id) {
                return Err(format!(
                    "拒绝会话级放行 optionId={}，仅允许单次 allow-once / reject-once",
                    option_id
                ));
            }
            Ok(PermissionPlan::SendToGrokStdin)
        }
        "agy" => {
            if option_id == "dismiss" || option_id == "reject-once" {
                return Ok(PermissionPlan::AcknowledgeAgyDenied);
            }
            return Err(
                "agy 官方 headless 已拒绝该操作，本窗口无法放行。请改用官方 Antigravity 桌面端。"
                    .to_string(),
            );
        }
        other => Err(format!("不支持的后端类型: {}", other)),
    }
}

/// Map an agy `step_update` object to a GUI permission card.
/// Field names come from official stream-json docs + a harmless run_command probe:
/// `event=step_update`, `step_update.step_type=tool`, `tool_name`, `tool_info.parameters`,
/// `tool_info.error.type/message`. Headless auto-denies Ask tools; stdin only accepts `user`.
fn agy_permission_from_step(
    session_id: &str,
    step: &Value,
) -> Option<PermissionRequestPayload> {
    let step_type = step.get("step_type").and_then(|s| s.as_str()).unwrap_or("");
    if step_type != "tool" {
        return None;
    }
    let tool_name = step
        .get("tool_name")
        .and_then(|s| s.as_str())
        .unwrap_or("tool");
    let tool_info = step.get("tool_info").cloned().unwrap_or(Value::Null);
    let err_msg = tool_info
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let is_ask_tool = tool_name == "ask_permission" || tool_name == "ask_custom_permission";
    let is_denied = err_msg.contains("permission check failed")
        || err_msg.contains("user denied permission");
    if !is_ask_tool && !is_denied {
        return None;
    }
    let params = tool_info.get("parameters");
    let command = params
        .and_then(|p| p.get("CommandLine").or_else(|| p.get("command")))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            if err_msg.is_empty() {
                None
            } else {
                Some(err_msg.to_string())
            }
        });
    Some(PermissionRequestPayload {
        id: step.get("step_index").cloned().unwrap_or(Value::Null),
        session_id: session_id.to_string(),
        tool_name: tool_name.to_string(),
        title: format!("CLI 已拒绝: {}", tool_name),
        command,
        options: agy_denied_options(),
        already_denied: true,
    })
}

pub async fn respond_permission(
    state: &SessionState,
    request_id: Value,
    option_id: String,
) -> Result<(), String> {
    let lock = state.active.lock().await;
    let Some(session) = lock.as_ref() else {
        return Err("当前无等待权限审批的会话".to_string());
    };
    match plan_permission_response(&session.backend, &option_id)? {
        PermissionPlan::SendToGrokStdin => {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option_id
                    }
                }
            });
            let line = resp.to_string() + "\n";
            session
                .stdin_tx
                .send(line)
                .await
                .map_err(|e| format!("发送权限决策失败: {}", e))?;
            Ok(())
        }
        PermissionPlan::AcknowledgeAgyDenied => {
            // Headless stdin only accepts `event: user`. Do not invent a grant.
            Ok(())
        }
    }
}

pub fn build_grok_agent_args(model: Option<&str>, reasoning_effort: Option<&str>) -> Vec<String> {
    let mut args = vec!["agent".to_string()];
    if let Some(m) = model {
        let m = m.trim();
        if !m.is_empty() {
            args.push("-m".to_string());
            args.push(m.to_string());
        }
    }
    if let Some(e) = reasoning_effort {
        let e = e.trim();
        if !e.is_empty() && e != "default" {
            args.push("--reasoning-effort".to_string());
            args.push(e.to_string());
        }
    }
    args.push("stdio".to_string());
    args
}

pub fn build_agy_args(model: Option<&str>, reasoning_effort: Option<&str>) -> Vec<String> {
    let mut args = vec![];
    if let Some(m) = model {
        let m = m.trim();
        if !m.is_empty() {
            args.push("--model".to_string());
            args.push(m.to_string());
        }
    }
    if let Some(e) = reasoning_effort {
        let e = e.trim();
        if !e.is_empty() && e != "default" {
            args.push("--effort".to_string());
            args.push(e.to_string());
        }
    }
    args.push("--input-format".to_string());
    args.push("stream-json".to_string());
    args.push("--output-format".to_string());
    args.push("stream-json".to_string());
    args
}

fn resolve_workspace_dir(workspace: &str) -> Result<PathBuf, String> {
    let trimmed = workspace.trim();
    if trimmed.is_empty() {
        return Err("请先选择工作区".to_string());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_dir() {
        return Err(format!("工作区路径不存在: {:?}", path));
    }
    Ok(path)
}

pub async fn start_task(
    app: AppHandle,
    state: &SessionState,
    backend: String,
    workspace: String,
    prompt: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<String, String> {
    let workspace_path = resolve_workspace_dir(&workspace)?;

    // 1. 终止已有会话
    stop_active_session(state).await;

    let home = home_dir()?.to_string_lossy().to_string();
    let session_uuid = Uuid::new_v4().to_string();

    match backend.as_str() {
        "grok" => {
            let grok_path = find_binary(&[&format!("{}/.grok/bin/grok", home)], "grok")
                .ok_or_else(|| "未找到 grok 二进制文件".to_string())?;

            let mut cmd = Command::new(grok_path);
            let args = build_grok_agent_args(model.as_deref(), reasoning_effort.as_deref());
            cmd.args(&args);
            cmd.current_dir(&workspace_path);
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| format!("启动 grok agent 失败: {}", e))?;
            let stdout = child.stdout.take().ok_or("无法捕获 stdout")?;
            let mut stdin = child.stdin.take().ok_or("无法捕获 stdin")?;
            spawn_stderr_drain(child.stderr.take());

            let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(32);

            // Stdin writer task
            tokio::spawn(async move {
                while let Some(line) = stdin_rx.recv().await {
                    if stdin.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                    if stdin.flush().await.is_err() {
                        break;
                    }
                }
            });

            // Save active session
            {
                let mut lock = state.active.lock().await;
                *lock = Some(ActiveSession {
                    backend: "grok".to_string(),
                    session_uuid: session_uuid.clone(),
                    child,
                    stdin_tx: stdin_tx.clone(),
                });
            }

            let app_clone = app.clone();
            let session_id_clone = session_uuid.clone();
            let workspace_str = workspace_path.to_string_lossy().to_string();

            // Reader loop
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();

                // Step 1: send initialize
                let init_req = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": 1,
                        "clientInfo": {
                            "name": "px-agent-gui",
                            "version": "0.1.0"
                        }
                    }
                });
                let _ = stdin_tx.send(init_req.to_string() + "\n").await;

                // Step 2: send session/new
                let new_session_req = json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "session/new",
                    "params": {
                        "cwd": workspace_str,
                        "mcpServers": []
                    }
                });
                let _ = stdin_tx.send(new_session_req.to_string() + "\n").await;

                let mut _grok_session_id: Option<String> = None;
                let mut prompt_sent = false;

                while let Ok(Some(line)) = reader.next_line().await {
                    if line.trim().is_empty() {
                        continue;
                    }

                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        let id = v.get("id").and_then(|i| i.as_i64());
                        let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");

                        // Capture sessionId from session/new response
                        if id == Some(2) {
                            if let Some(res) = v.get("result") {
                                if let Some(sid) = res.get("sessionId").and_then(|s| s.as_str()) {
                                    _grok_session_id = Some(sid.to_string());

                                    // Turn off always-approve so user permissions are prompted
                                    let aa_req = json!({
                                        "jsonrpc": "2.0",
                                        "id": 21,
                                        "method": "session/prompt",
                                        "params": {
                                            "sessionId": sid,
                                            "prompt": [{"type": "text", "text": "/always-approve off"}]
                                        }
                                    });
                                    let _ = stdin_tx.send(aa_req.to_string() + "\n").await;

                                    // Send the actual prompt
                                    let prompt_req = json!({
                                        "jsonrpc": "2.0",
                                        "id": 3,
                                        "method": "session/prompt",
                                        "params": {
                                            "sessionId": sid,
                                            "prompt": [{"type": "text", "text": prompt}]
                                        }
                                    });
                                    let _ = stdin_tx.send(prompt_req.to_string() + "\n").await;
                                    prompt_sent = true;
                                }
                            }
                        }

                        // Stream updates
                        if method == "session/update" {
                            if let Some(params) = v.get("params") {
                                if let Some(update) = params.get("update") {
                                    let update_type = update
                                        .get("sessionUpdate")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("");

                                    if update_type == "agent_message_chunk" {
                                        if let Some(text) = update
                                            .get("content")
                                            .and_then(|c| c.get("text"))
                                            .and_then(|t| t.as_str())
                                        {
                                            let _ = app_clone.emit(
                                                "session_update",
                                                StreamUpdatePayload {
                                                    session_id: session_id_clone.clone(),
                                                    kind: "text_delta".to_string(),
                                                    text: Some(text.to_string()),
                                                    tool_name: None,
                                                    title: None,
                                                },
                                            );
                                        }
                                    } else if update_type == "agent_thought_chunk" {
                                        if let Some(text) = update
                                            .get("content")
                                            .and_then(|c| c.get("text"))
                                            .and_then(|t| t.as_str())
                                        {
                                            let _ = app_clone.emit(
                                                "session_update",
                                                StreamUpdatePayload {
                                                    session_id: session_id_clone.clone(),
                                                    kind: "thought_delta".to_string(),
                                                    text: Some(text.to_string()),
                                                    tool_name: None,
                                                    title: None,
                                                },
                                            );
                                        }
                                    } else if update_type == "tool_call" {
                                        let tool_title = update
                                            .get("title")
                                            .and_then(|t| t.as_str())
                                            .map(|s| s.to_string());
                                        let _ = app_clone.emit(
                                            "session_update",
                                            StreamUpdatePayload {
                                                session_id: session_id_clone.clone(),
                                                kind: "tool_call".to_string(),
                                                text: None,
                                                tool_name: None,
                                                title: tool_title,
                                            },
                                        );
                                    }
                                }
                            }
                        }

                        // Permission request
                        if method == "session/request_permission" {
                            let req_id = v.get("id").cloned().unwrap_or(Value::Null);
                            let params = v.get("params").cloned().unwrap_or(Value::Null);
                            let sid = params
                                .get("sessionId")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            let tool_call = params.get("toolCall");
                            let tool_title = tool_call
                                .and_then(|t| t.get("title"))
                                .and_then(|s| s.as_str())
                                .unwrap_or("请求工具执行权限")
                                .to_string();
                            let tool_name = tool_call
                                .and_then(|t| t.get("kind"))
                                .and_then(|s| s.as_str())
                                .unwrap_or("command")
                                .to_string();
                            let raw_cmd = tool_call
                                .and_then(|t| t.get("rawInput"))
                                .and_then(|r| r.get("command"))
                                .and_then(|c| c.as_str())
                                .map(|s| s.to_string());

                            let _ = app_clone.emit(
                                "permission_request",
                                PermissionRequestPayload {
                                    id: req_id,
                                    session_id: sid,
                                    tool_name,
                                    title: tool_title,
                                    command: raw_cmd,
                                    options: grok_permission_options_for_emit(&params),
                                    already_denied: false,
                                },
                            );
                        }

                        // Terminal prompt response
                        if prompt_sent && id == Some(3) {
                            let _ = app_clone.emit(
                                "session_end",
                                SessionEndPayload {
                                    session_id: session_id_clone.clone(),
                                    status: "COMPLETED".to_string(),
                                    full_response: None,
                                },
                            );
                            break;
                        }
                    }
                }
            });

            Ok(session_uuid)
        }
        "agy" => {
            let agy_path = find_binary(&[&format!("{}/.local/bin/agy", home)], "agy")
                .ok_or_else(|| "未找到 agy 二进制文件".to_string())?;

            let mut cmd = Command::new(agy_path);
            let args = build_agy_args(model.as_deref(), reasoning_effort.as_deref());
            cmd.args(&args);
            cmd.current_dir(&workspace_path);
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| format!("启动 agy 进程失败: {}", e))?;
            let stdout = child.stdout.take().ok_or("无法捕获 stdout")?;
            let mut stdin = child.stdin.take().ok_or("无法捕获 stdin")?;
            spawn_stderr_drain(child.stderr.take());

            let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(32);

            tokio::spawn(async move {
                while let Some(line) = stdin_rx.recv().await {
                    if stdin.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                    if stdin.flush().await.is_err() {
                        break;
                    }
                }
            });

            {
                let mut lock = state.active.lock().await;
                *lock = Some(ActiveSession {
                    backend: "agy".to_string(),
                    session_uuid: session_uuid.clone(),
                    child,
                    stdin_tx: stdin_tx.clone(),
                });
            }

            let app_clone = app.clone();
            let session_id_clone = session_uuid.clone();

            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();

                // Wait for `init` event before sending prompt
                let mut init_seen = false;
                let mut full_resp_buf = String::new();

                while let Ok(Some(line)) = reader.next_line().await {
                    if line.trim().is_empty() {
                        continue;
                    }

                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("");

                        if event == "init" && !init_seen {
                            init_seen = true;
                            // Send user prompt in agy stream-json format
                            let user_msg = json!({
                                "event": "user",
                                "message": {
                                    "content": prompt
                                }
                            });
                            let _ = stdin_tx.send(user_msg.to_string() + "\n").await;
                        } else if event == "step_update" {
                            if let Some(step) = v.get("step_update") {
                                let step_type = step
                                    .get("step_type")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("");

                                if let Some(delta) =
                                    step.get("text_delta").and_then(|t| t.as_str())
                                {
                                    full_resp_buf.push_str(delta);
                                    let _ = app_clone.emit(
                                        "session_update",
                                        StreamUpdatePayload {
                                            session_id: session_id_clone.clone(),
                                            kind: "text_delta".to_string(),
                                            text: Some(delta.to_string()),
                                            tool_name: None,
                                            title: None,
                                        },
                                    );
                                }

                                if step_type == "tool" {
                                    let tool_name = step
                                        .get("tool_name")
                                        .and_then(|t| t.as_str())
                                        .map(|s| s.to_string());
                                    let state = step
                                        .get("state")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("");
                                    let _ = app_clone.emit(
                                        "session_update",
                                        StreamUpdatePayload {
                                            session_id: session_id_clone.clone(),
                                            kind: "tool_call".to_string(),
                                            text: None,
                                            tool_name,
                                            title: Some(format!("工具执行状态: {}", state)),
                                        },
                                    );
                                    if let Some(perm) =
                                        agy_permission_from_step(&session_id_clone, step)
                                    {
                                        let _ = app_clone.emit("permission_request", perm);
                                    }
                                }
                            }
                        } else if event == "result" {
                            let resp_text = v
                                .get("result")
                                .and_then(|r| r.get("response"))
                                .and_then(|t| t.as_str())
                                .unwrap_or("");

                            let final_content = if resp_text.is_empty() {
                                full_resp_buf.clone()
                            } else {
                                resp_text.to_string()
                            };

                            let _ = app_clone.emit(
                                "session_end",
                                SessionEndPayload {
                                    session_id: session_id_clone.clone(),
                                    status: "COMPLETED".to_string(),
                                    full_response: Some(final_content),
                                },
                            );
                            break;
                        }
                    }
                }
            });

            Ok(session_uuid)
        }
        _ => Err(format!("不支持的后端类型: {}", backend)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_workspace_is_rejected() {
        let err = resolve_workspace_dir("").unwrap_err();
        assert!(err.contains("请先选择工作区"));
        assert!(resolve_workspace_dir("   ").is_err());
    }

    #[test]
    fn missing_or_non_dir_workspace_is_rejected() {
        let missing = std::env::temp_dir().join(format!(
            "px-agent-gui-missing-ws-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&missing);
        let err = resolve_workspace_dir(missing.to_str().unwrap()).unwrap_err();
        assert!(err.contains("工作区路径不存在"));

        let file = std::env::temp_dir().join(format!(
            "px-agent-gui-ws-file-{}",
            std::process::id()
        ));
        std::fs::write(&file, b"x").unwrap();
        assert!(resolve_workspace_dir(file.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn existing_workspace_dir_is_accepted() {
        let root = std::env::temp_dir().join(format!(
            "px-agent-gui-ws-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let got = resolve_workspace_dir(root.to_str().unwrap()).unwrap();
        assert_eq!(got, root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_session_wide_allow_ids() {
        assert!(is_session_wide_allow("allow-edits-session"));
        assert!(is_session_wide_allow("allow-always"));
        assert!(is_session_wide_allow("allow_always"));
        assert!(!is_session_wide_allow("allow-once"));
        assert!(!is_session_wide_allow("reject-once"));
        assert!(is_once_option("allow-once"));
        assert!(is_once_option("reject-once"));
        assert!(!is_once_option("allow-edits-session"));
    }

    #[test]
    fn agy_denied_run_command_is_informational_card_without_allow() {
        let step = json!({
            "conversation_id": "c8e473af-14e9-4bf8-88e8-e72497e9aa94",
            "step_index": 4,
            "state": "ERROR",
            "step_type": "tool",
            "tool_name": "run_command",
            "tool_info": {
                "name": "run_command",
                "parameters": {"CommandLine": "echo AGY_PERM_PROBE > /tmp/px_agy_perm_probe.txt"},
                "error": {
                    "type": "TOOL_ERROR",
                    "message": "permission check failed for command \"echo AGY_PERM_PROBE\": user denied permission to run command"
                }
            }
        });
        let perm = agy_permission_from_step("sess-1", &step).expect("denied tool should emit card");
        assert_eq!(perm.tool_name, "run_command");
        assert_eq!(perm.command.as_deref(), Some("echo AGY_PERM_PROBE > /tmp/px_agy_perm_probe.txt"));
        assert!(perm.already_denied);
        assert!(perm.title.starts_with("CLI 已拒绝"));
        assert_eq!(
            perm.options
                .iter()
                .map(|o| o.option_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dismiss"]
        );
        assert!(!perm.options.iter().any(|o| {
            o.option_id == "allow-once" || o.kind.as_deref() == Some("allow_once")
        }));
        assert!(!perm.options.iter().any(|o| is_session_wide_allow(&o.option_id)));
    }

    #[test]
    fn agy_successful_tool_does_not_emit_permission_card() {
        let step = json!({
            "step_index": 2,
            "state": "DONE",
            "step_type": "tool",
            "tool_name": "run_command",
            "tool_info": {
                "name": "run_command",
                "parameters": {"CommandLine": "echo hi"},
                "output": "hi\n"
            }
        });
        assert!(agy_permission_from_step("sess-1", &step).is_none());
    }

    #[test]
    fn agy_ask_permission_tool_emits_denied_card_without_allow() {
        let step = json!({
            "step_index": 3,
            "state": "ACTIVE",
            "step_type": "tool",
            "tool_name": "ask_permission",
            "tool_info": {"name": "ask_permission", "parameters": {}}
        });
        let perm = agy_permission_from_step("sess-1", &step).expect("ask_permission should emit card");
        assert_eq!(perm.tool_name, "ask_permission");
        assert!(perm.already_denied);
        assert_eq!(perm.options[0].option_id, "dismiss");
        assert!(!perm.options.iter().any(|o| o.option_id == "allow-once"));
    }

    #[test]
    fn grok_permission_emit_keeps_only_once_options() {
        let params = json!({
            "options": [
                {
                    "optionId": "allow-edits-session",
                    "name": "Allow all edits this session",
                    "kind": "allow_always"
                },
                {
                    "optionId": "allow-once",
                    "name": "Allow once",
                    "kind": "allow_once"
                },
                {
                    "optionId": "allow_always",
                    "name": "Always allow",
                    "kind": "allow_always"
                },
                {
                    "optionId": "reject-once",
                    "name": "Reject",
                    "kind": "reject_once"
                }
            ]
        });
        let options = grok_permission_options_for_emit(&params);
        let ids: Vec<&str> = options.iter().map(|o| o.option_id.as_str()).collect();
        assert_eq!(ids, vec!["allow-once", "reject-once"]);
        assert!(!options.iter().any(|o| is_session_wide_allow(&o.option_id)));
        assert!(!options.iter().any(|o| {
            let id = o.option_id.to_ascii_lowercase();
            id.contains("session") || id.contains("always")
        }));
    }

    #[test]
    fn grok_once_permission_plan_unchanged() {
        assert_eq!(
            plan_permission_response("grok", "allow-once").unwrap(),
            PermissionPlan::SendToGrokStdin
        );
        assert_eq!(
            plan_permission_response("grok", "reject-once").unwrap(),
            PermissionPlan::SendToGrokStdin
        );
        assert!(plan_permission_response("grok", "allow-edits-session").is_err());
        assert!(plan_permission_response("grok", "allow-always").is_err());
        assert!(plan_permission_response("grok", "dismiss").is_err());
    }

    #[test]
    fn agy_allow_is_not_reported_as_granted() {
        let err = plan_permission_response("agy", "allow-once").unwrap_err();
        assert!(err.contains("无法放行"), "allow must not look like a grant: {err}");
        assert!(!err.contains("成功"));
        assert!(plan_permission_response("agy", "allow-always").is_err());
        assert_eq!(
            plan_permission_response("agy", "dismiss").unwrap(),
            PermissionPlan::AcknowledgeAgyDenied
        );
        assert_eq!(
            plan_permission_response("agy", "reject-once").unwrap(),
            PermissionPlan::AcknowledgeAgyDenied
        );
    }

    #[test]
    fn build_grok_agent_args_places_model_and_effort_before_stdio() {
        let default_args = build_grok_agent_args(None, None);
        assert_eq!(default_args, vec!["agent", "stdio"]);

        let custom_args = build_grok_agent_args(Some("grok-4.5"), Some("low"));
        assert_eq!(
            custom_args,
            vec!["agent", "-m", "grok-4.5", "--reasoning-effort", "low", "stdio"]
        );

        let effort_default_skipped = build_grok_agent_args(Some("grok-4.6"), Some("default"));
        assert_eq!(effort_default_skipped, vec!["agent", "-m", "grok-4.6", "stdio"]);

        let joined = custom_args.join(" ");
        assert!(!joined.contains("always-approve"));
        assert!(!joined.contains("dangerously-skip-permissions"));
    }

    #[test]
    fn build_agy_args_preserves_stream_json_and_never_skips_permissions() {
        let args = build_agy_args(Some("gemini-3.8-flash-high"), None);
        assert_eq!(
            args,
            vec![
                "--model",
                "gemini-3.8-flash-high",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json"
            ]
        );

        let joined = args.join(" ");
        assert!(!joined.contains("dangerously-skip-permissions"));
        assert!(!joined.contains("always"));
    }
}
