#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod client;
use client::{ApiError, ConnectionInfo, Desktop};
use serde_json::Value;
#[tauri::command]
async fn api_request(
    state: tauri::State<'_, Desktop>,
    path: String,
    method: String,
    body: Value,
) -> Result<Value, ApiError> {
    state.request(path, method, body).await
}
#[tauri::command]
async fn connection_info(state: tauri::State<'_, Desktop>) -> Result<ConnectionInfo, ApiError> {
    Ok(state.info().await?)
}
#[tauri::command]
async fn set_connection(
    state: tauri::State<'_, Desktop>,
    url: String,
    token: String,
) -> Result<ConnectionInfo, ApiError> {
    Ok(state.set(url, token).await?)
}
fn main() {
    let desktop = Desktop::new().expect("BiBi 설정을 읽을 수 없습니다.");
    tauri::Builder::default()
        .manage(desktop)
        .invoke_handler(tauri::generate_handler![
            api_request,
            connection_info,
            set_connection
        ])
        .run(tauri::generate_context!())
        .expect("BiBi 앱 실행 오류");
}
