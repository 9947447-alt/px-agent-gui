use crate::session::resolve_workspace_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const STORE_FILE_NAME: &str = "projects-sessions.json";
pub const STORE_VERSION: u32 = 1;
pub const DEFAULT_SESSION_TITLE: &str = "新对话";
pub const TITLE_MAX_CHARS: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalProject {
    pub id: String,
    pub name: String,
    pub workspace_path: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalToolCall {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<LocalToolCall>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalSession {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub title_custom: bool,
    pub backend: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: String,
    #[serde(default)]
    pub messages: Vec<LocalMessage>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalStore {
    pub version: u32,
    #[serde(default)]
    pub projects: Vec<LocalProject>,
    #[serde(default)]
    pub sessions: Vec<LocalSession>,
    #[serde(default)]
    pub active_project_id: Option<String>,
    #[serde(default)]
    pub active_session_id: Option<String>,
}

pub fn store_file_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(STORE_FILE_NAME)
}

pub fn empty_store() -> LocalStore {
    LocalStore {
        version: STORE_VERSION,
        projects: Vec::new(),
        sessions: Vec::new(),
        active_project_id: None,
        active_session_id: None,
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn title_from_first_user_message(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return DEFAULT_SESSION_TITLE.to_string();
    }
    collapsed.chars().take(TITLE_MAX_CHARS).collect()
}

pub fn normalize_store(store: &mut LocalStore) {
    for session in &mut store.sessions {
        if session.title.trim().is_empty() {
            session.title = DEFAULT_SESSION_TITLE.to_string();
        }
        if !session.title_custom {
            if let Some(msg) = session.messages.iter().find(|m| m.role == "user") {
                session.title = title_from_first_user_message(&msg.content);
            }
        }
    }
}

pub fn validate_store_shape(store: &LocalStore) -> Result<(), String> {
    let mut project_ids = HashSet::new();
    for project in &store.projects {
        if project.id.trim().is_empty() {
            return Err("项目 id 不能为空".to_string());
        }
        if project.name.trim().is_empty() {
            return Err("项目名称不能为空".to_string());
        }
        if project.workspace_path.trim().is_empty() {
            return Err("项目工作区路径不能为空".to_string());
        }
        if !project_ids.insert(project.id.clone()) {
            return Err(format!("重复的项目 id: {}", project.id));
        }
    }

    let mut session_ids = HashSet::new();
    for session in &store.sessions {
        if session.id.trim().is_empty() {
            return Err("会话 id 不能为空".to_string());
        }
        if !session_ids.insert(session.id.clone()) {
            return Err(format!("重复的会话 id: {}", session.id));
        }
        if !project_ids.contains(&session.project_id) {
            return Err(format!(
                "会话 {} 未绑定到已有项目（projectId={}）",
                session.id, session.project_id
            ));
        }
        if session.backend != "grok" && session.backend != "agy" {
            return Err(format!("不支持的后端类型: {}", session.backend));
        }
    }

    if let Some(pid) = &store.active_project_id {
        if !project_ids.contains(pid) {
            return Err("当前项目不存在".to_string());
        }
    }
    if let Some(sid) = &store.active_session_id {
        if !session_ids.contains(sid) {
            return Err("当前会话不存在".to_string());
        }
        let session = store
            .sessions
            .iter()
            .find(|s| &s.id == sid)
            .expect("session id checked");
        if let Some(pid) = &store.active_project_id {
            if &session.project_id != pid {
                return Err("当前会话不属于当前项目".to_string());
            }
        }
    }
    Ok(())
}

pub fn load_store(path: &Path) -> Result<LocalStore, String> {
    if !path.exists() {
        return Ok(empty_store());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取本地项目数据失败: {}", e))?;
    if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(empty_store());
    }
    let store: LocalStore =
        serde_json::from_slice(&bytes).map_err(|e| format!("解析本地项目数据失败: {}", e))?;
    validate_store_shape(&store)?;
    Ok(store)
}

pub fn save_store(path: &Path, store: &mut LocalStore) -> Result<(), String> {
    validate_store_shape(store)?;
    normalize_store(store);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建应用数据目录失败: {}", e))?;
    }
    let data =
        serde_json::to_vec_pretty(store).map_err(|e| format!("序列化本地项目数据失败: {}", e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &data).map_err(|e| format!("写入本地项目数据失败: {}", e))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("保存本地项目数据失败: {}", e))?;
    Ok(())
}

pub fn create_project(
    store: &mut LocalStore,
    name: &str,
    workspace: &str,
    now: i64,
    id: String,
) -> Result<LocalProject, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("项目名称不能为空".to_string());
    }
    let workspace_path = resolve_workspace_dir(workspace)?;
    let project = LocalProject {
        id,
        name: name.to_string(),
        workspace_path: workspace_path.to_string_lossy().to_string(),
        created_at: now,
    };
    store.projects.push(project.clone());
    store.active_project_id = Some(project.id.clone());
    store.active_session_id = None;
    Ok(project)
}

pub fn create_session(
    store: &mut LocalStore,
    project_id: &str,
    backend: &str,
    model: &str,
    reasoning_effort: &str,
    now: i64,
    id: String,
) -> Result<LocalSession, String> {
    if !store.projects.iter().any(|p| p.id == project_id) {
        return Err("请先选择项目".to_string());
    }
    if backend != "grok" && backend != "agy" {
        return Err(format!("不支持的后端类型: {}", backend));
    }
    let session = LocalSession {
        id,
        project_id: project_id.to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        title_custom: false,
        backend: backend.to_string(),
        model: model.to_string(),
        reasoning_effort: reasoning_effort.to_string(),
        messages: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    store.sessions.push(session.clone());
    store.active_project_id = Some(project_id.to_string());
    store.active_session_id = Some(session.id.clone());
    Ok(session)
}

pub fn sessions_for_project<'a>(store: &'a LocalStore, project_id: &str) -> Vec<&'a LocalSession> {
    let mut list: Vec<&LocalSession> = store
        .sessions
        .iter()
        .filter(|s| s.project_id == project_id)
        .collect();
    list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    list
}

pub fn rename_session(store: &mut LocalStore, session_id: &str, title: &str) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("会话标题不能为空".to_string());
    }
    let session = store
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
        .ok_or_else(|| "会话不存在".to_string())?;
    session.title = title.to_string();
    session.title_custom = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "px-agent-gui-store-{}-{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn empty_store_has_no_hardcoded_default_project() {
        let store = empty_store();
        assert!(store.projects.is_empty());
        assert!(store.sessions.is_empty());
        assert!(store.active_project_id.is_none());
        assert!(store.active_session_id.is_none());
        let json = serde_json::to_string(&store).unwrap();
        assert!(!json.to_ascii_lowercase().contains("reaction-field"));
        assert!(!json.contains("/Users/a0000"));
    }

    #[test]
    fn store_file_lives_under_app_data_not_repo() {
        let app_data = PathBuf::from("/tmp/px-agent-gui-app-data");
        let path = store_file_path(&app_data);
        assert_eq!(path, app_data.join("projects-sessions.json"));
        assert!(!path.to_string_lossy().contains("px-agent-gui/src"));
    }

    #[test]
    fn create_project_does_not_fall_back_to_home() {
        let mut store = empty_store();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/a0000".to_string());
        let err = create_project(&mut store, "demo", "", 1, "p1".into()).unwrap_err();
        assert!(err.contains("请先选择工作区"));
        assert!(store.projects.is_empty());
        assert!(!store.projects.iter().any(|p| p.workspace_path == home));
        assert_ne!(
            create_project(&mut store, "demo", "   ", 1, "p2".into()).unwrap_err(),
            home
        );
    }

    #[test]
    fn create_project_requires_existing_directory_and_name() {
        let mut store = empty_store();
        assert!(create_project(&mut store, "  ", "/tmp", 1, "p1".into()).is_err());

        let missing = std::env::temp_dir().join(format!(
            "px-agent-gui-missing-project-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&missing);
        let err = create_project(
            &mut store,
            "demo",
            missing.to_str().unwrap(),
            1,
            "p1".into(),
        )
        .unwrap_err();
        assert!(err.contains("工作区路径不存在"));
        assert!(store.projects.is_empty());
    }

    #[test]
    fn sessions_are_bound_to_project_via_project_id() {
        let ws = temp_workspace("bind");
        let mut store = empty_store();
        let project = create_project(
            &mut store,
            "开发目录",
            ws.to_str().unwrap(),
            10,
            "proj-1".into(),
        )
        .unwrap();
        assert_eq!(store.active_project_id.as_deref(), Some("proj-1"));
        assert!(store.active_session_id.is_none());

        let a = create_session(
            &mut store,
            &project.id,
            "grok",
            "grok-4.6",
            "high",
            11,
            "sess-a".into(),
        )
        .unwrap();
        let b =
            create_session(&mut store, &project.id, "agy", "", "", 12, "sess-b".into()).unwrap();
        assert_eq!(a.project_id, project.id);
        assert_eq!(b.project_id, project.id);
        assert_eq!(a.title, "新对话");
        assert_eq!(b.title, "新对话");

        let listed = sessions_for_project(&store, &project.id);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "sess-b");
        assert_eq!(listed[1].id, "sess-a");

        assert!(create_session(&mut store, "missing", "grok", "", "", 13, "x".into()).is_err());
        cleanup(&ws);
    }

    #[test]
    fn auto_title_truncates_first_user_message_and_rename_sticks() {
        assert_eq!(
            title_from_first_user_message("  hello   world  "),
            "hello world"
        );
        let long = "测".repeat(50);
        assert_eq!(title_from_first_user_message(&long).chars().count(), 40);
        assert_eq!(title_from_first_user_message("   "), "新对话");

        let ws = temp_workspace("title");
        let mut store = empty_store();
        create_project(&mut store, "p", ws.to_str().unwrap(), 1, "proj-1".into()).unwrap();
        create_session(&mut store, "proj-1", "grok", "", "", 2, "sess-1".into()).unwrap();
        store.sessions[0].messages.push(LocalMessage {
            id: "m1".into(),
            role: "user".into(),
            content: "  帮我检查这段代码是否有问题  ".into(),
            thoughts: None,
            tool_call: None,
            created_at: 3,
        });
        normalize_store(&mut store);
        assert_eq!(store.sessions[0].title, "帮我检查这段代码是否有问题");

        rename_session(&mut store, "sess-1", "审计记录").unwrap();
        store.sessions[0].messages.push(LocalMessage {
            id: "m2".into(),
            role: "user".into(),
            content: "第二条消息不应该改标题".into(),
            thoughts: None,
            tool_call: None,
            created_at: 4,
        });
        normalize_store(&mut store);
        assert_eq!(store.sessions[0].title, "审计记录");
        assert!(store.sessions[0].title_custom);
        cleanup(&ws);
    }

    #[test]
    fn save_and_load_roundtrip_keeps_two_named_chats() {
        let root = temp_workspace("roundtrip");
        let ws = root.join("workspace");
        std::fs::create_dir_all(&ws).unwrap();
        let path = store_file_path(&root.join("app-data"));

        let mut store = empty_store();
        create_project(
            &mut store,
            "开发",
            ws.to_str().unwrap(),
            100,
            "proj-1".into(),
        )
        .unwrap();
        create_session(
            &mut store,
            "proj-1",
            "grok",
            "grok-4.6",
            "low",
            101,
            "s1".into(),
        )
        .unwrap();
        create_session(
            &mut store,
            "proj-1",
            "grok",
            "grok-4.6",
            "low",
            102,
            "s2".into(),
        )
        .unwrap();
        store.sessions[0].messages.push(LocalMessage {
            id: "u1".into(),
            role: "user".into(),
            content: "第一条聊天的问题".into(),
            thoughts: None,
            tool_call: None,
            created_at: 103,
        });
        store.sessions[1].messages.push(LocalMessage {
            id: "u2".into(),
            role: "user".into(),
            content: "第二条聊天的问题".into(),
            thoughts: None,
            tool_call: None,
            created_at: 104,
        });

        save_store(&path, &mut store).unwrap();
        assert!(path.exists());
        assert!(path.starts_with(&root));

        let loaded = load_store(&path).unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "开发");
        assert_eq!(loaded.projects[0].workspace_path, ws.to_string_lossy());
        assert_eq!(loaded.sessions.len(), 2);
        assert_eq!(loaded.sessions[0].project_id, "proj-1");
        assert_eq!(loaded.sessions[1].project_id, "proj-1");
        assert_eq!(loaded.sessions[0].title, "第一条聊天的问题");
        assert_eq!(loaded.sessions[1].title, "第二条聊天的问题");
        assert_eq!(loaded.active_session_id.as_deref(), Some("s2"));
        assert!(!serde_json::to_string(&loaded)
            .unwrap()
            .to_ascii_lowercase()
            .contains("reaction-field"));

        let missing = load_store(&root.join("no-such.json")).unwrap();
        assert_eq!(missing, empty_store());
        cleanup(&root);
    }

    #[test]
    fn save_rejects_session_not_bound_to_project() {
        let root = temp_workspace("orphan");
        let path = store_file_path(&root);
        let mut store = empty_store();
        store.sessions.push(LocalSession {
            id: "orphan".into(),
            project_id: "missing".into(),
            title: "x".into(),
            title_custom: false,
            backend: "grok".into(),
            model: String::new(),
            reasoning_effort: String::new(),
            messages: vec![],
            created_at: 1,
            updated_at: 1,
        });
        let err = save_store(&path, &mut store).unwrap_err();
        assert!(err.contains("未绑定到已有项目"));
        assert!(!path.exists());
        cleanup(&root);
    }
}
