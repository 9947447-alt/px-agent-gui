pub mod detector;
pub mod session;

use detector::{open_desktop_app as handle_open_desktop_app, probe_all, SystemStatus};
use serde_json::Value;
use session::{respond_permission as handle_permission, start_task as handle_start_task, stop_active_session, SessionState};
use tauri::{AppHandle, State};

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

pub fn run() {
    tauri::Builder::default()
        .manage(SessionState::new())
        .invoke_handler(tauri::generate_handler![
            probe_status,
            start_task,
            respond_permission,
            stop_session,
            open_desktop_app,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用程序时发生错误");
}
