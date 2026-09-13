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
    pub kayg: DesktopAppStatus,
    pub antigravity: DesktopAppStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppStatus {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub installed: bool,
    pub path: Option<String>,
    pub install_hint: String,
}

const KAYG_APP_NAMES: &[&str] = &[
    "KayG.app",
    "Grok GUI.app",
    "Grok Build GUI.app",
    "Grok Build Desktop.app",
];

const KAYG_BUNDLE_IDS: &[&str] = &[
    "ai.grok.build.gui",
    "com.productcompass.grok-build-desktop",
];

const ANTIGRAVITY_APP_NAMES: &[&str] = &["Antigravity.app"];
const ANTIGRAVITY_BUNDLE_IDS: &[&str] = &["com.google.antigravity"];

fn home_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Common macOS app locations. Uses $HOME, never a hardcoded username.
pub fn app_search_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = home {
        dirs.push(home.join("Applications"));
    }
    dirs
}

pub fn filter_app_bundles(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.extension().and_then(|e| e.to_str()) != Some("app") {
            continue;
        }
        if p.exists() {
            out.push(p);
        }
    }
    out
}

fn find_named_app(names: &[&str], dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        for name in names {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn pick_preferred_app(found: &[PathBuf], preferred_names: &[&str]) -> Option<PathBuf> {
    for name in preferred_names {
        if let Some(p) = found.iter().find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == *name)
                .unwrap_or(false)
        }) {
            return Some(p.clone());
        }
    }
    found.first().cloned()
}

fn mdfind_bundle_query(bundle_ids: &[&str]) -> String {
    let clauses: Vec<String> = bundle_ids
        .iter()
        .map(|id| format!("kMDItemCFBundleIdentifier == '{}'", id))
        .collect();
    format!(
        "kMDItemContentType == 'com.apple.application-bundle' && ({})",
        clauses.join(" || ")
    )
}

fn mdfind_app_paths(query: &str) -> Vec<PathBuf> {
    let output = std::process::Command::new("mdfind").arg(query).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    filter_app_bundles(text.lines().map(|l| PathBuf::from(l.trim())).filter(|p| {
        !p.as_os_str().is_empty()
    }))
}

fn locate_desktop_app(names: &[&str], bundle_ids: &[&str]) -> Option<PathBuf> {
    let dirs = app_search_dirs(home_path().as_deref());
    if let Some(found) = find_named_app(names, &dirs) {
        return Some(found);
    }
    let from_mdfind = mdfind_app_paths(&mdfind_bundle_query(bundle_ids));
    pick_preferred_app(&from_mdfind, names)
}

fn desktop_status(
    id: &str,
    name: &str,
    purpose: &str,
    path: Option<PathBuf>,
    install_hint: &str,
) -> DesktopAppStatus {
    match path {
        Some(p) => DesktopAppStatus {
            id: id.to_string(),
            name: name.to_string(),
            purpose: purpose.to_string(),
            installed: true,
            path: Some(p.to_string_lossy().to_string()),
            install_hint: install_hint.to_string(),
        },
        None => DesktopAppStatus {
            id: id.to_string(),
            name: name.to_string(),
            purpose: purpose.to_string(),
            installed: false,
            path: None,
            install_hint: install_hint.to_string(),
        },
    }
}

pub fn probe_kayg() -> DesktopAppStatus {
    desktop_status(
        "kayg",
        "KayG / Grok Build GUI",
        "审计",
        locate_desktop_app(KAYG_APP_NAMES, KAYG_BUNDLE_IDS),
        "未检测到 KayG / Grok Build GUI。日常 Grok 请安装该桌面端；本仓不替代它。",
    )
}

pub fn probe_antigravity_app() -> DesktopAppStatus {
    desktop_status(
        "antigravity",
        "Antigravity",
        "实现",
        locate_desktop_app(ANTIGRAVITY_APP_NAMES, ANTIGRAVITY_BUNDLE_IDS),
        "未检测到官方 Antigravity 桌面。日常 Gemini 请安装官方桌面端；本仓不替代它。",
    )
}

fn existing_workspace_dir(workspace: &str) -> Option<PathBuf> {
    let trimmed = workspace.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = PathBuf::from(trimmed);
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// Arguments for `/usr/bin/open -a <app> [workspace]`. Never a skip-permissions flag.
pub fn open_app_args(app_path: &Path, workspace: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        "-a".to_string(),
        app_path.to_string_lossy().to_string(),
    ];
    if let Some(ws) = workspace {
        args.push(ws.to_string_lossy().to_string());
    }
    args
}

fn run_open(args: &[String]) -> Result<(), String> {
    let output = std::process::Command::new("open")
        .args(args)
        .output()
        .map_err(|e| format!("无法执行 open: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "open 未能打开应用".to_string()
        } else {
            stderr
        })
    }
}

pub fn open_desktop_app(app_id: &str, workspace: &str) -> Result<String, String> {
    let status = match app_id {
        "kayg" => probe_kayg(),
        "antigravity" => probe_antigravity_app(),
        _ => return Err("未知桌面应用。仅支持 kayg 或 antigravity。".to_string()),
    };
    if !status.installed {
        return Err(status.install_hint);
    }
    let app_path = status
        .path
        .as_deref()
        .ok_or_else(|| status.install_hint.clone())?;
    let app_path = Path::new(app_path);
    let ws = existing_workspace_dir(workspace);
    if let Some(ws) = ws.as_deref() {
        let with_ws = open_app_args(app_path, Some(ws));
        if run_open(&with_ws).is_ok() {
            return Ok(format!("已打开 {}（附带工作区）", status.name));
        }
    }
    let app_only = open_app_args(app_path, None);
    run_open(&app_only)?;
    Ok(format!("已打开 {}", status.name))
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
    SystemStatus {
        grok,
        agy,
        kayg: probe_kayg(),
        antigravity: probe_antigravity_app(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_dirs_use_home_not_hardcoded_user() {
        let dirs = app_search_dirs(Some(Path::new("/tmp/someone")));
        assert!(dirs.contains(&PathBuf::from("/Applications")));
        assert!(dirs.contains(&PathBuf::from("/tmp/someone/Applications")));
        assert!(dirs
            .iter()
            .all(|d| !d.to_string_lossy().contains("/Users/a0000")));
    }

    #[test]
    fn filter_keeps_only_existing_app_bundles() {
        let root = std::env::temp_dir().join(format!(
            "px-agent-gui-app-test-{}",
            std::process::id()
        ));
        let app = root.join("Grok GUI.app");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&app).unwrap();
        let hits = filter_app_bundles([
            app.clone(),
            root.join("not-an-app"),
            PathBuf::from("/tmp/does-not-exist.app"),
        ]);
        assert_eq!(hits, vec![app.clone()]);
        let found = find_named_app(&["Grok GUI.app", "KayG.app"], &[root.clone()]);
        assert_eq!(found.as_deref(), Some(app.as_path()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mdfind_query_is_bundle_id_not_user_path() {
        let q = mdfind_bundle_query(KAYG_BUNDLE_IDS);
        assert!(q.contains("ai.grok.build.gui"));
        assert!(q.contains("com.productcompass.grok-build-desktop"));
        assert!(!q.contains("/Users/a0000"));
        let q2 = mdfind_bundle_query(ANTIGRAVITY_BUNDLE_IDS);
        assert!(q2.contains("com.google.antigravity"));
        assert!(!q2.contains("/Users/a0000"));
    }

    #[test]
    fn open_args_are_macos_open_dash_a() {
        let app = Path::new("/Applications/Grok GUI.app");
        let ws = Path::new("/tmp/workspace");
        assert_eq!(
            open_app_args(app, Some(ws)),
            vec![
                "-a".to_string(),
                "/Applications/Grok GUI.app".to_string(),
                "/tmp/workspace".to_string()
            ]
        );
        assert_eq!(
            open_app_args(app, None),
            vec!["-a".to_string(), "/Applications/Grok GUI.app".to_string()]
        );
        let joined = open_app_args(app, Some(ws)).join(" ");
        assert!(!joined.contains("dangerously-skip-permissions"));
        assert!(!joined.contains("allow-always"));
    }

    #[test]
    fn unknown_desktop_app_id_is_rejected() {
        let err = open_desktop_app("codex-router", "").unwrap_err();
        assert!(err.contains("未知桌面应用"));
    }

    #[test]
    fn prefers_grok_gui_name_over_later_hits() {
        let picked = pick_preferred_app(
            &[
                PathBuf::from("/Applications/Grok Build Desktop.app"),
                PathBuf::from("/Applications/Grok GUI.app"),
            ],
            KAYG_APP_NAMES,
        );
        assert_eq!(
            picked.as_deref(),
            Some(Path::new("/Applications/Grok GUI.app"))
        );
    }
}
