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

pub async fn respond_permission(
    state: &SessionState,
    request_id: Value,
    option_id: String,
) -> Result<(), String> {
    if is_session_wide_allow(&option_id) {
        return Err(format!(
            "拒绝会话级放行 optionId={}，仅允许单次 allow-once / reject-once",
            option_id
        ));
    }
    let lock = state.active.lock().await;
    if let Some(session) = lock.as_ref() {
        if session.backend == "grok" {
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
            return Ok(());
        }
    }
    Err("当前无等待权限审批的 Grok 会话".to_string())
}

pub async fn start_task(
    app: AppHandle,
    state: &SessionState,
    backend: String,
    workspace: String,
    prompt: String,
) -> Result<String, String> {
    // 1. 终止已有会话
    stop_active_session(state).await;

    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/a0000".to_string());
    let session_uuid = Uuid::new_v4().to_string();

    let workspace_path = if workspace.trim().is_empty() {
        PathBuf::from(&home)
    } else {
        PathBuf::from(workspace.trim())
    };

    if !workspace_path.exists() {
        return Err(format!("工作区路径不存在: {:?}", workspace_path));
    }

    match backend.as_str() {
        "grok" => {
            let grok_path = find_binary(&[&format!("{}/.grok/bin/grok", home)], "grok")
                .ok_or_else(|| "未找到 grok 二进制文件".to_string())?;

            let mut cmd = Command::new(grok_path);
            cmd.arg("agent").arg("stdio");
            cmd.current_dir(&workspace_path);
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| format!("启动 grok agent 失败: {}", e))?;
            let stdout = child.stdout.take().ok_or("无法捕获 stdout")?;
            let mut stdin = child.stdin.take().ok_or("无法捕获 stdin")?;

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

                            let mut options = vec![];
                            if let Some(opts) = params.get("options").and_then(|o| o.as_array()) {
                                for opt in opts {
                                    if let Some(opt_id) =
                                        opt.get("optionId").and_then(|s| s.as_str())
                                    {
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

                            let _ = app_clone.emit(
                                "permission_request",
                                PermissionRequestPayload {
                                    id: req_id,
                                    session_id: sid,
                                    tool_name,
                                    title: tool_title,
                                    command: raw_cmd,
                                    options,
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
            cmd.arg("--input-format").arg("stream-json");
            cmd.arg("--output-format").arg("stream-json");
            cmd.current_dir(&workspace_path);
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| format!("启动 agy 进程失败: {}", e))?;
            let stdout = child.stdout.take().ok_or("无法捕获 stdout")?;
            let mut stdin = child.stdin.take().ok_or("无法捕获 stdin")?;

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
