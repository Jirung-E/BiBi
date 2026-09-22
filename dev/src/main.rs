use std::{path::Path, process::Command};

fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let profile_dir = executable.parent().ok_or("빌드 출력 폴더가 없습니다.")?;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/run-desktop.mjs");
    let status = Command::new("node")
        .arg(script)
        .arg(profile_dir)
        .args(std::env::args_os().skip(1))
        .status()
        .map_err(|error| format!("소스 실행에는 Node.js와 npm이 필요합니다: {error}"))?;
    Ok(status.code().unwrap_or(1))
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => std::process::ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
