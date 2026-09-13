use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub backend: String,
    pub installed: bool,
    pub logged_in: bool,
    pub path: Option<String>,
    pub models: Vec<String>,
    pub install_command: String,
    pub login_command: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub grok: CliStatus,
    pub agy: CliStatus,
}

pub fn find_binary(candidates: &[&str], cmd_name: &str) -> Option<PathBuf> {
    for cand in candidates {
        let p = Path::new(cand);
        if p.exists() && p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    // Also check which cmd_name
    if let Ok(output) = std::process::Command::new("which").arg(cmd_name).output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                let p = PathBuf::from(s);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub async fn probe_grok() -> CliStatus {
    let home = std::env::var("HOME").ok();
    let default_path = home
        .as_ref()
        .map(|h| format!("{}/.grok/bin/grok", h));
    let binary = match default_path.as_deref() {
        Some(p) => find_binary(&[p], "grok"),
        None => find_binary(&[], "grok"),
    };

    let install_cmd = "curl -fsSL https://x.ai/cli/install.sh | bash".to_string();
    let login_cmd = "grok login".to_string();

    match binary {
        None => CliStatus {
            backend: "grok".to_string(),
            installed: false,
            logged_in: false,
            path: None,
            models: vec![],
            install_command: install_cmd,
            login_command: login_cmd,
            error: Some("找不到 grok 二进制文件".to_string()),
        },
        Some(path_buf) => {
            let path_str = path_buf.to_string_lossy().to_string();
            // Test `grok models`
            let output = Command::new(&path_buf)
                .arg("models")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await;

            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let mut models = vec![];
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with('*') || trimmed.starts_with('-') {
                            let name = trimmed.trim_start_matches(|c| c == '*' || c == '-' || c == ' ')
                                .split_whitespace()
                                .next()
                                .unwrap_or("")
                                .to_string();
                            if !name.is_empty() {
                                models.push(name);
                            }
                        }
                    }
                    if models.is_empty() {
                        models.push("grok-4.6".to_string());
                    }
                    CliStatus {
                        backend: "grok".to_string(),
                        installed: true,
                        logged_in: true,
                        path: Some(path_str),
                        models,
                        install_command: install_cmd,
                        login_command: login_cmd,
                        error: None,
                    }
                }
                Ok(out) => {
                    let err_msg = String::from_utf8_lossy(&out.stderr).to_string();
                    CliStatus {
                        backend: "grok".to_string(),
                        installed: true,
                        logged_in: false,
                        path: Some(path_str),
                        models: vec![],
                        install_command: install_cmd,
                        login_command: login_cmd,
                        error: Some(if err_msg.is_empty() { "未登录".to_string() } else { err_msg }),
                    }
                }
                Err(e) => CliStatus {
                    backend: "grok".to_string(),
                    installed: true,
                    logged_in: false,
                    path: Some(path_str),
                    models: vec![],
                    install_command: install_cmd,
                    login_command: login_cmd,
                    error: Some(format!("执行 grok models 失败: {}", e)),
                },
            }
        }
    }
}

pub async fn probe_agy() -> CliStatus {
    let home = std::env::var("HOME").ok();
    let default_path = home
        .as_ref()
        .map(|h| format!("{}/.local/bin/agy", h));
    let binary = match default_path.as_deref() {
        Some(p) => find_binary(&[p], "agy"),
        None => find_binary(&[], "agy"),
    };

    let install_cmd = "curl -fsSL https://antigravity.google/cli/install.sh | bash".to_string();
    let login_cmd = "agy".to_string();

    match binary {
        None => CliStatus {
            backend: "agy".to_string(),
            installed: false,
            logged_in: false,
            path: None,
            models: vec![],
            install_command: install_cmd,
            login_command: login_cmd,
            error: Some("找不到 agy 二进制文件".to_string()),
        },
        Some(path_buf) => {
            let path_str = path_buf.to_string_lossy().to_string();
            // Test `agy models`
            let output = Command::new(&path_buf)
                .arg("models")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await;

            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let mut models = vec![];
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if trimmed.contains('\t') {
                            if let Some(first) = trimmed.split('\t').next() {
                                if !first.is_empty() {
                                    models.push(first.to_string());
                                }
                            }
                        }
                    }
                    if models.is_empty() {
                        models.push("gemini-3.8-flash-high".to_string());
                    }
                    CliStatus {
                        backend: "agy".to_string(),
                        installed: true,
                        logged_in: true,
                        path: Some(path_str),
                        models,
                        install_command: install_cmd,
                        login_command: login_cmd,
                        error: None,
                    }
                }
                Ok(out) => {
                    let err_msg = String::from_utf8_lossy(&out.stderr).to_string();
                    CliStatus {
                        backend: "agy".to_string(),
                        installed: true,
                        logged_in: false,
                        path: Some(path_str),
                        models: vec![],
                        install_command: install_cmd,
                        login_command: login_cmd,
                        error: Some(if err_msg.is_empty() { "未登录".to_string() } else { err_msg }),
                    }
                }
                Err(e) => CliStatus {
                    backend: "agy".to_string(),
                    installed: true,
                    logged_in: false,
                    path: Some(path_str),
                    models: vec![],
                    install_command: install_cmd,
                    login_command: login_cmd,
                    error: Some(format!("执行 agy models 失败: {}", e)),
                },
            }
        }
    }
}

pub async fn probe_all() -> SystemStatus {
    let (grok, agy) = tokio::join!(probe_grok(), probe_agy());
    SystemStatus { grok, agy }
}
