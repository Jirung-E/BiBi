#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod client;
#[cfg(target_os = "macos")]
mod macos;
mod server_lifetime;
#[cfg(windows)]
mod tray;
use client::{ApiError, ConnectionInfo, Desktop};
use serde_json::Value;
#[tauri::command]
async fn api_request(
    _app: tauri::AppHandle,
    state: tauri::State<'_, Desktop>,
    path: String,
    method: String,
    body: Value,
) -> Result<Value, ApiError> {
    #[cfg(windows)]
    let check_connection = path == "/api/snapshot" || path.starts_with("/api/events");
    let result = state.request(path, method, body).await;
    #[cfg(windows)]
    if check_connection && let Some(info) = state.cached_info().await {
        tray::update(&_app, &info, result.is_ok());
    }
    result
}
#[tauri::command]
async fn connection_info(
    _app: tauri::AppHandle,
    state: tauri::State<'_, Desktop>,
) -> Result<ConnectionInfo, ApiError> {
    let info = state.info().await?;
    #[cfg(windows)]
    tray::update(&_app, &info, true);
    Ok(info)
}
#[tauri::command]
async fn set_connection(
    _app: tauri::AppHandle,
    state: tauri::State<'_, Desktop>,
    url: String,
    token: String,
) -> Result<ConnectionInfo, ApiError> {
    let info = state.set(url, token).await?;
    #[cfg(windows)]
    tray::update(&_app, &info, true);
    Ok(info)
}
#[tauri::command]
fn set_window_chrome(window: tauri::WebviewWindow, left: f64, center_y: f64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::update(window, left, center_y);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, left, center_y);
        Ok(())
    }
}
fn main() {
    let desktop = Desktop::new().expect("BiBi 설정을 읽을 수 없습니다.");
    let context = tauri::generate_context!();
    #[cfg(windows)]
    let context = {
        let mut context = context;
        // Separate data directories/explicit remote targets can coexist. A
        // second launch of the same target restores its existing tray window.
        context.config_mut().identifier = desktop
            .instance_id()
            .expect("BiBi 실행 대상을 읽을 수 없습니다.");
        context
    };
    let builder = tauri::Builder::default();
    #[cfg(windows)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            tray::show(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(tray::window_event);
    builder
        .manage(desktop)
        .setup(|_app| {
            #[cfg(target_os = "macos")]
            macos::install(_app)?;
            #[cfg(windows)]
            tray::install(_app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api_request,
            connection_info,
            set_connection,
            set_window_chrome
        ])
        .build(context)
        .expect("BiBi 앱 실행 오류")
        .run(|_app, _event| {
            #[cfg(windows)]
            if let tauri::RunEvent::ExitRequested { api, .. } = _event {
                tray::exiting(_app, &api);
            }
        });
}
