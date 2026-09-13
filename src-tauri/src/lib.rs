pub mod detector;
pub mod session;
pub mod store;

use detector::{open_desktop_app as handle_open_desktop_app, probe_all, SystemStatus};
use serde_json::Value;
use session::{
    respond_permission as handle_permission, start_task as handle_start_task, stop_active_session,
    SessionState,
};
use store::{
    create_project, create_session, load_store, now_ms, save_store, store_file_path, LocalStore,
};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[tauri::command]
async fn probe_status() -> Result<SystemStatus, String> {
    Ok(probe_all().await)
}

#[tauri::command]
fn open_desktop_app(app_id: String, workspace: String) -> Result<String, String> {
    handle_open_desktop_app(&app_id, &workspace)
}

#[tauri::command]
async fn start_task(
    app: AppHandle,
    state: State<'_, SessionState>,
    backend: String,
    workspace: String,
    prompt: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<String, String> {
    handle_start_task(app, &state, backend, workspace, prompt, model, reasoning_effort).await
}

#[tauri::command]
async fn respond_permission(
    state: State<'_, SessionState>,
    request_id: Value,
    option_id: String,
) -> Result<(), String> {
    handle_permission(&state, request_id, option_id).await
}

#[tauri::command]
async fn stop_session(state: State<'_, SessionState>) -> Result<(), String> {
    stop_active_session(&state).await;
    Ok(())
}

fn local_store_file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法解析应用数据目录: {}", e))?;
    Ok(store_file_path(&dir))
}

#[tauri::command]
fn load_local_store(app: AppHandle) -> Result<LocalStore, String> {
    load_store(&local_store_file(&app)?)
}

#[tauri::command]
fn save_local_store(app: AppHandle, mut store: LocalStore) -> Result<LocalStore, String> {
    save_store(&local_store_file(&app)?, &mut store)?;
    Ok(store)
}

#[tauri::command]
fn create_local_project(
    app: AppHandle,
    name: String,
    workspace: String,
) -> Result<LocalStore, String> {
    let path = local_store_file(&app)?;
    let mut store = load_store(&path)?;
    create_project(
        &mut store,
        &name,
        &workspace,
        now_ms(),
        Uuid::new_v4().to_string(),
    )?;
    save_store(&path, &mut store)?;
    Ok(store)
}

#[tauri::command]
fn create_local_session(
    app: AppHandle,
    project_id: String,
    backend: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<LocalStore, String> {
    let path = local_store_file(&app)?;
    let mut store = load_store(&path)?;
    create_session(
        &mut store,
        &project_id,
        &backend,
        model.as_deref().unwrap_or(""),
        reasoning_effort.as_deref().unwrap_or(""),
        now_ms(),
        Uuid::new_v4().to_string(),
    )?;
    save_store(&path, &mut store)?;
    Ok(store)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(SessionState::new())
        .invoke_handler(tauri::generate_handler![
            probe_status,
            start_task,
            respond_permission,
            stop_session,
            open_desktop_app,
            load_local_store,
            save_local_store,
            create_local_project,
            create_local_session,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用程序时发生错误");
}
