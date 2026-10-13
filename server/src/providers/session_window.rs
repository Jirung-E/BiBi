//! Focus only a verified external native process. Never send terminal input.
use super::{claude_history, claude_live};
use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::Value;
#[cfg(any(windows, target_os = "macos"))]
use serde_json::json;

pub async fn open(engine: &Engine, id: &str) -> Result<Value> {
    let run = engine.store.run(id)?;
    if run.host_id != "local"
        || run.origin != Origin::External
        || run.provider != Provider::Claude
        || run.agent_kind == "subagent"
    {
        bail!("이 호스트에서 실행 중인 외부 Claude 루트 세션의 창만 열 수 있습니다.");
    }
    tokio::task::spawn_blocking(move || {
        let (root, _) = claude_history::resume_source(&run)?;
        let current = claude_live::registrations(&root)?
            .remove(run.session_key.as_deref().unwrap_or(""))
            .context("실행 중인 Claude 프로세스를 찾지 못했습니다.")?;
        if !current.verified
            || !claude_live::same_workspace(&current.registration.cwd, &run.workspace)
        {
            bail!("원래 Claude 프로세스와 작업 폴더를 확인할 수 없습니다.");
        }
        focus(&current.registration)
    })
    .await?
}
#[cfg(target_os = "macos")]
pub(crate) fn focus(registration: &claude_live::Registration) -> Result<Value> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &registration.pid.to_string(), "-o", "tty="])
        .output()?;
    let tty = String::from_utf8(output.stdout)?.trim().to_string();
    if !output.status.success()
        || !tty.starts_with("ttys")
        || !tty.chars().all(|c| c.is_ascii_alphanumeric())
    {
        bail!(
            "원래 세션의 터미널 장치를 찾지 못했습니다. IDE 내장 터미널과 백그라운드 실행은 지원하지 않습니다."
        );
    }
    // The TTY is passed as data. The fixed script selects only an existing tab.
    let script = r#"on run argv
set targetTTY to "/dev/" & item 1 of argv
if application "Terminal" is running then
 tell application "Terminal"
  repeat with w in windows
   repeat with t in tabs of w
    if tty of t is targetTTY then
     set selected tab of w to t
     set miniaturized of w to false
     set index of w to 1
     activate
     return "Terminal"
    end if
   end repeat
  end repeat
 end tell
end if
error "No matching terminal tab"
end run"#;
    let result = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", script, &tty])
        .output()?;
    if !result.status.success() {
        bail!(
            "해당 Terminal 탭을 열지 못했습니다. 원래 탭이 열려 있는지와 macOS 자동화 권한을 확인하세요."
        );
    }
    Ok(json!({"status":"focused","message":"원래 터미널 탭을 열었습니다."}))
}
#[cfg(windows)]
pub(crate) fn focus(registration: &claude_live::Registration) -> Result<Value> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    let created = registration
        .proc_start_ft
        .as_deref()
        .context("Windows 프로세스 생성 시각을 확인할 수 없습니다.")?;
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--bibi-focus-console",
            &registration.pid.to_string(),
            created,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code();
        }
        if std::time::Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            bail!("원래 창 확인 시간이 초과되었습니다.");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    match code {
        Some(0) => Ok(
            json!({"status":"focused","message":"원래 터미널 창을 열었습니다. Windows Terminal의 탭 선택은 직접 해 주세요."}),
        ),
        Some(2) => Ok(
            json!({"status":"attention","message":"Windows가 자동 포커스를 제한했습니다. 작업 표시줄에서 강조된 원래 터미널 창을 선택하세요."}),
        ),
        Some(4) => {
            bail!("Claude 프로세스가 바뀌었거나 종료되었습니다. 세션 목록을 새로고침하세요.")
        }
        _ => bail!(
            "이 터미널의 원래 창을 식별할 수 없습니다. IDE 내장 터미널·SSH·일부 Windows Terminal 버전은 지원하지 않습니다."
        ),
    }
}
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn focus(_: &claude_live::Registration) -> Result<Value> {
    bail!("이 운영체제의 터미널 창 연결은 아직 지원하지 않습니다.")
}

/// Runs before either executable starts its app/runtime. Attaching a helper
/// avoids detaching or changing the BiBi server's own console and stdio.
pub fn helper_entry() -> Option<i32> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "--bibi-focus-console") {
        return None;
    }
    #[cfg(windows)]
    {
        let pid = args.get(2).and_then(|s| s.parse::<u32>().ok());
        let created = args.get(3).and_then(|s| s.parse::<u64>().ok());
        Some(match (pid, created) {
            (Some(pid), Some(created)) if args.len() == 4 => windows_console(pid, created),
            _ => 4,
        })
    }
    #[cfg(not(windows))]
    {
        Some(3)
    }
}
#[cfg(windows)]
fn windows_console(pid: u32, created: u64) -> i32 {
    use windows_sys::Win32::{
        System::Console::{AttachConsole, FreeConsole, GetConsoleProcessList, GetConsoleWindow},
        UI::WindowsAndMessaging::{
            FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, GA_ROOT, GW_OWNER,
            GetAncestor, GetForegroundWindow, GetWindow, IsWindow, IsWindowVisible, SW_RESTORE,
            SetForegroundWindow, ShowWindowAsync,
        },
    };
    if claude_live::windows_process_start(pid) != Ok(Some(created)) {
        return 4;
    }
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return 3;
        }
        let result = (|| {
            let mut processes = [0u32; 512];
            let count =
                GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32) as usize;
            if count == 0
                || count > processes.len()
                || !processes[..count].contains(&pid)
                || claude_live::windows_process_start(pid) != Ok(Some(created))
            {
                return 4;
            }
            let console = GetConsoleWindow();
            if console.is_null() {
                return 3;
            }
            let owner = GetWindow(console, GW_OWNER);
            let window = GetAncestor(if owner.is_null() { console } else { owner }, GA_ROOT);
            if window.is_null() || IsWindow(window) == 0 || IsWindowVisible(window) == 0 {
                return 3;
            }
            ShowWindowAsync(window, SW_RESTORE);
            if SetForegroundWindow(window) != 0 || GetForegroundWindow() == window {
                return 0;
            }
            let info = FLASHWINFO {
                cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                hwnd: window,
                dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
                uCount: 3,
                dwTimeout: 0,
            };
            FlashWindowEx(&info);
            2
        })();
        FreeConsole();
        result
    }
}
