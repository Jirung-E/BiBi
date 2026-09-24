use crate::client::{ConnectionInfo, Desktop};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{
    AppHandle, Manager, WindowEvent,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

struct TrayState {
    status: MenuItem<tauri::Wry>,
    address: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    last: Mutex<String>,
    requesting: AtomicBool,
    finished: AtomicBool,
}

pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "BiBi 열기", true, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", "연결 준비 중", false, None::<&str>)?;
    let address = MenuItem::with_id(app, "address", "", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "BiBi 종료", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &status, &address, &separator, &quit])?;
    let mut builder = TrayIconBuilder::with_id("bibi")
        .menu(&menu)
        .tooltip("BiBi · 연결 준비 중")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show(app),
            "quit" => request_exit(app),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(TrayState {
        status,
        address,
        quit,
        last: Mutex::new(String::new()),
        requesting: AtomicBool::new(false),
        finished: AtomicBool::new(false),
    });
    Ok(())
}

pub fn window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() == "main"
        && let WindowEvent::CloseRequested { api, .. } = event
    {
        // The tray has to exist before hiding the only visible window.
        if window.app_handle().try_state::<TrayState>().is_some() {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

pub fn update(app: &AppHandle, info: &ConnectionInfo, connected: bool) {
    let state = app.state::<TrayState>();
    let location = if info.mode == "local" {
        "로컬 서버"
    } else {
        "원격 서버"
    };
    let status = format!(
        "{} · {}",
        location,
        if connected {
            "연결됨"
        } else {
            "연결 끊김"
        }
    );
    let key = format!("{status}:{}:{}", info.url, info.managed_local);
    let mut last = state.last.lock().unwrap();
    if *last == key {
        return;
    }
    *last = key;
    let _ = state.status.set_text(&status);
    let _ = state.address.set_text(&info.url);
    let _ = state.quit.set_text(if info.managed_local {
        "BiBi 종료 (로컬 서버 포함)"
    } else {
        "앱 종료 (서버 유지)"
    });
    if let Some(tray) = app.tray_by_id("bibi") {
        let _ = tray.set_tooltip(Some(format!("BiBi · {status}\n{}", info.url)));
    }
}

async fn confirm(app: &AppHandle, message: String) -> bool {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title("BiBi 종료")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "종료".into(),
            "취소".into(),
        ))
        .show(move |confirmed| {
            let _ = sender.send(confirmed);
        });
    receiver.await.unwrap_or(false)
}

pub fn request_exit(app: &AppHandle) {
    let state = app.state::<TrayState>();
    if state.requesting.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = state.quit.set_enabled(false);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let desktop = app.state::<Desktop>();
        let confirmed = match desktop.owned_work_count().await {
            Ok(Some(n)) if n > 0 => {
                confirm(
                    &app,
                    format!(
                        "진행 중인 작업 {n}개가 있습니다. 로컬 서버와 작업을 중단하고 종료할까요?"
                    ),
                )
                .await
            }
            Err(_) => confirm(
                &app,
                "로컬 서버의 상태를 확인할 수 없습니다. 서버에 종료를 요청하고 앱을 종료할까요?"
                    .into(),
            )
            .await,
            _ => true,
        };
        if confirmed {
            match desktop.shutdown_owned().await {
                Ok(()) => {
                    app.state::<TrayState>()
                        .finished
                        .store(true, Ordering::Release);
                    app.exit(0);
                    return;
                }
                Err(error) => {
                    show(&app);
                    app.dialog()
                        .message(error.to_string())
                        .title("BiBi 종료")
                        .kind(MessageDialogKind::Error)
                        .show(|_| {});
                }
            }
        }
        let state = app.state::<TrayState>();
        state.requesting.store(false, Ordering::Release);
        let _ = state.quit.set_enabled(true);
    });
}

pub fn exiting(app: &AppHandle, api: &tauri::ExitRequestApi) {
    if !app.state::<TrayState>().finished.load(Ordering::Acquire) {
        api.prevent_exit();
        request_exit(app);
    }
}
