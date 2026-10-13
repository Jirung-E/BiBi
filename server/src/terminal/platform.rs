use super::*;
fn executable() -> Result<PathBuf> {
    let current = std::env::current_exe()?;
    let sibling = current
        .parent()
        .context("실행 파일 위치 없음")?
        .join(if cfg!(windows) { "bibi.exe" } else { "bibi" });
    if sibling.is_file() {
        return Ok(sibling);
    }
    if current.file_stem().is_some_and(|s| s == "bibi") {
        return Ok(current);
    }
    bail!("터미널용 bibi 실행 파일이 없습니다. just build로 CLI와 앱을 함께 빌드하세요.")
}
pub(super) fn open(data_dir: &Path, ticket: &str) -> Result<()> {
    let exe = executable()?;
    let data_dir = std::path::absolute(data_dir)?;
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // Launch Services opens a private .command file. Unlike AppleScript
        // automation, this ordinary user-initiated document action does not need
        // permission to control all of Terminal's windows.
        uuid::Uuid::parse_str(ticket)?;
        let directory = data_dir.join("terminal-launches");
        std::fs::create_dir_all(&directory)?;
        if std::fs::symlink_metadata(&directory)?
            .file_type()
            .is_symlink()
        {
            bail!("터미널 실행 폴더가 심볼릭 링크입니다.");
        }
        let path = directory.join(format!("{ticket}.command"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&path)?;
        writeln!(
            file,
            "#!/bin/sh\nexec {} --bibi-terminal-run {} {}",
            shell_quote(&exe.to_string_lossy()),
            shell_quote(&data_dir.to_string_lossy()),
            shell_quote(ticket)
        )?;
        drop(file);
        let mut child = std::process::Command::new("/usr/bin/open")
            .args(["-a", "Terminal", "--"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    let _ = std::fs::remove_file(&path);
                    bail!(
                        "Terminal을 열지 못했습니다. 기본 Terminal 앱과 로그인 상태를 확인하세요."
                    );
                }
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&path);
                bail!(
                    "Terminal 열기 요청 시간이 초과됐습니다. Terminal 상태를 확인한 뒤 다시 여세요."
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new(exe)
            .args(["--bibi-terminal-run"])
            .arg(data_dir)
            .arg(ticket)
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE)
            .spawn()?;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let candidates = std::env::var("TERMINAL").ok().into_iter().chain([
            "x-terminal-emulator".into(),
            "gnome-terminal".into(),
            "konsole".into(),
            "xterm".into(),
        ]);
        let mut started = false;
        for terminal in candidates {
            let Ok(resolved) = crate::providers::launch::describe(&terminal) else {
                continue;
            };
            if resolved.entrypoint.is_some() {
                continue;
            }
            let separator = if Path::new(&terminal)
                .file_name()
                .is_some_and(|n| n == "gnome-terminal")
            {
                "--"
            } else {
                "-e"
            };
            std::process::Command::new(resolved.executable)
                .arg(separator)
                .arg(&exe)
                .arg("--bibi-terminal-run")
                .arg(&data_dir)
                .arg(ticket)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            started = true;
            break;
        }
        if !started {
            bail!("지원되는 터미널을 찾지 못했습니다. TERMINAL에 실행 파일 경로를 지정하세요.");
        }
    }
    Ok(())
}
pub(super) fn stdio(_cmd: &mut tokio::process::Command) -> Result<()> {
    #[cfg(windows)]
    {
        // The console CLI is started with CREATE_NEW_CONSOLE by the GUI.
        // Opening devices also handles a parent GUI with null inherited handles.
        if unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() }.is_null() {
            return Ok(());
        }
        let input = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("CONIN$")?;
        let output = std::fs::OpenOptions::new().write(true).open("CONOUT$")?;
        _cmd.stdin(input).stdout(output.try_clone()?).stderr(output);
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
#[cfg(test)]
#[test]
fn shell_arguments_are_quoted_as_single_literal_values() {
    assert_eq!(
        shell_quote("space ' ; $(whoami)\nnext"),
        "'space '\"'\"' ; $(whoami)\nnext'"
    );
}
