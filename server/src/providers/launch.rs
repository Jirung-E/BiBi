//! Resolve installed CLIs consistently for probes, sessions, and quota refreshes.
use anyhow::Result;
use std::{
    ffi::OsStr,
    fmt, io,
    path::{Path, PathBuf},
};
use tokio::process::{Child, Command};

#[derive(Debug)]
pub(crate) struct LaunchError(String);
impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LaunchError {}

pub(crate) fn command(program: &str) -> Result<Command> {
    let paths = search_paths();
    let binary = resolve(program, &paths, cfg!(windows)).ok_or_else(|| {
        LaunchError("CLI 실행 파일을 찾지 못했습니다. BiBi 서버 PC의 CLI 설치를 확인하고 앱·서버를 재시작하거나 실행 파일에 전체 경로를 입력하세요.".into())
    })?;
    let mut command = if cfg!(windows)
        && let Some(entry) = npm_entrypoint(&binary)
    {
        // npm's .cmd wrapper adds a shell boundary that cannot faithfully carry
        // multiline prompts/JSON. Invoke its known Node entry point directly.
        let sibling = binary.parent().unwrap().join("node.exe");
        let node = if sibling.is_file() {
            sibling
        } else {
            resolve("node", &paths, true)
                .filter(|p| extension_is(p, "exe"))
                .ok_or_else(|| LaunchError("npm CLI에 필요한 Node.js 실행 파일을 찾지 못했습니다. BiBi 서버 PC의 Node.js 설치와 PATH를 확인하세요.".into()))?
        };
        let mut command = Command::new(node);
        command.arg(entry);
        command
    } else {
        Command::new(binary)
    };
    command.env("PATH", std::env::join_paths(paths)?);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // No transient console window in the GUI.
    Ok(command)
}

pub(crate) fn spawn(command: &mut Command) -> Result<Child> {
    command.spawn().map_err(|error| {
        let reason = match error.kind() {
            io::ErrorKind::NotFound => "CLI 실행 파일 또는 필요한 실행 환경을 찾지 못했습니다.",
            io::ErrorKind::PermissionDenied => "CLI 실행 권한이 없습니다.",
            io::ErrorKind::InvalidInput => "CLI 실행 파일·인자 형식을 처리할 수 없습니다.",
            _ => "CLI 프로세스를 시작할 수 없습니다.",
        };
        let code = error
            .raw_os_error()
            .map(|n| format!(" (OS 오류 {n})"))
            .unwrap_or_default();
        LaunchError(format!(
            "{reason}{code} BiBi 서버 PC의 CLI 설치와 실행 파일 설정을 확인하세요."
        ))
        .into()
    })
}

fn search_paths() -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        paths.push(parent.into());
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/bin"));
    }
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            paths.push(PathBuf::from(appdata).join("npm"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            paths.push(PathBuf::from(local).join("Microsoft/WinGet/Links"));
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            paths.push(PathBuf::from(program_files).join("nodejs"));
        }
        if let Some(nvm) = std::env::var_os("NVM_SYMLINK") {
            paths.push(nvm.into());
        }
    } else {
        paths.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
    }
    paths
}

fn resolve(program: &str, paths: &[PathBuf], windows: bool) -> Option<PathBuf> {
    let program = Path::new(program.trim());
    if program.as_os_str().is_empty() {
        return None;
    }
    let candidates = |base: PathBuf| {
        if windows && base.extension().is_none() {
            // Do not select npm's extensionless POSIX shim on Windows.
            ["exe", "com", "cmd", "bat"]
                .into_iter()
                .map(|ext| base.with_extension(ext))
                .collect()
        } else {
            vec![base]
        }
    };
    let files: Vec<PathBuf> = if program.is_absolute() || program.components().count() > 1 {
        candidates(program.into())
    } else {
        paths
            .iter()
            .flat_map(|dir| candidates(dir.join(program)))
            .collect()
    };
    files
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| std::path::absolute(p).ok())
}

fn extension_is(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

// Only bypass an unmodified npm-generated shim. User-written wrappers retain
// their behavior, including their arguments/environment/other commands.
fn npm_entrypoint(shim: &Path) -> Option<PathBuf> {
    if !extension_is(shim, "cmd") || shim.metadata().ok()?.len() > 64 * 1024 {
        return None;
    }
    let relative = match shim.file_stem()?.to_str()?.to_ascii_lowercase().as_str() {
        "codex" => "node_modules/@openai/codex/bin/codex.js",
        "claude" => "node_modules/@anthropic-ai/claude-code/cli.js",
        _ => return None,
    };
    let entry = shim.parent()?.join(relative);
    if !entry.is_file() {
        return None;
    }
    let source = std::fs::read_to_string(shim).ok()?;
    let normalized = |text: &str| {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase()
    };
    (normalized(&source) == normalized(&npm_shim(relative))).then_some(entry)
}

fn npm_shim(relative: &str) -> String {
    format!(
        r#"@ECHO off
GOTO start
:find_dp0
SET dp0=%~dp0
EXIT /b
:start
SETLOCAL
CALL :find_dp0

IF EXIST "%dp0%\node.exe" (
  SET "_prog=%dp0%\node.exe"
) ELSE (
  SET "_prog=node"
  SET PATHEXT=%PATHEXT:;.JS;=;%
)

endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\{}" %*
"#,
        relative.replace('/', "\\")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_names_find_npm_shims_and_native_files_in_path_order() {
        let dir = tempfile::tempdir().unwrap();
        let npm = dir.path().join("npm 한글 경로");
        let native = dir.path().join("native");
        std::fs::create_dir_all(&npm).unwrap();
        std::fs::create_dir_all(&native).unwrap();
        std::fs::write(npm.join("codex"), "#!/bin/sh").unwrap();
        std::fs::write(npm.join("codex.cmd"), "@echo fixture").unwrap();
        std::fs::write(native.join("codex.exe"), "fixture").unwrap();
        let paths = vec![npm.clone(), native.clone()];
        assert_eq!(resolve("codex", &paths, true), Some(npm.join("codex.cmd")));
        assert_eq!(
            resolve("codex.cmd", &paths, true),
            Some(npm.join("codex.cmd"))
        );
        std::fs::write(npm.join("codex.exe"), "fixture").unwrap();
        assert_eq!(resolve("codex", &paths, true), Some(npm.join("codex.exe")));
        assert_eq!(
            resolve(native.join("codex").to_str().unwrap(), &paths, true),
            Some(native.join("codex.exe"))
        );
        assert!(resolve("missing", &paths, true).is_none());
        assert!(
            resolve(
                dir.path().join("missing/codex").to_str().unwrap(),
                &paths,
                true
            )
            .is_none()
        );
        assert_eq!(resolve("codex", &paths, false), Some(npm.join("codex")));
    }

    #[test]
    fn only_standard_npm_wrappers_are_replaced_by_their_node_entrypoint() {
        let dir = tempfile::tempdir().unwrap();
        for (name, relative) in [
            ("codex", "node_modules/@openai/codex/bin/codex.js"),
            ("claude", "node_modules/@anthropic-ai/claude-code/cli.js"),
        ] {
            let entry = dir.path().join(relative);
            std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
            std::fs::write(&entry, "// fixture").unwrap();
            let shim = dir.path().join(format!("{name}.cmd"));
            let source = include_str!("../../tests/fixtures/npm-shim.cmd").replace(
                "PACKAGE\\ENTRY",
                &relative
                    .trim_start_matches("node_modules/")
                    .replace('/', "\\"),
            );
            std::fs::write(&shim, source.replace('\n', "\r\n")).unwrap();
            assert_eq!(npm_entrypoint(&shim), Some(entry));
            std::fs::write(&shim, format!("set EXTRA=custom\n{source}")).unwrap();
            assert!(npm_entrypoint(&shim).is_none());
        }
    }

    // Relaunch this test with an isolated PATH instead of changing process-wide
    // environment variables while the other tests are running in parallel.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_default_templates_launch_npm_clis_without_a_shell() {
        use crate::{config::ServiceConfig, providers::check::check, runtime::Engine};
        use bibi_core::{ProviderConfig, Store};
        use serde_json::{Value, json};
        use std::{process::Stdio, time::Duration};

        const RECORD: &str = "BIBI_WINDOWS_CLI_RECORD";
        if std::env::var_os(RECORD).is_some() {
            let dir = tempfile::tempdir().unwrap();
            let engine = Engine::new(
                Store::memory().unwrap(),
                ServiceConfig::new(dir.path().into()),
            );
            let literal = [
                "첫 줄\n둘째 줄\r\n끝",
                r#"{"mcpServers":{"bibi":{"type":"sdk"}}}"#,
                r#"spaces "quotes" %PATH% !NAME! & | < > ^ trailing\"#,
                "",
            ];
            for name in ["codex", "claude"] {
                let provider: ProviderConfig = serde_json::from_value(json!({
                    "id": "draft", "name": name, "adapter": name, "command": name
                }))
                .unwrap();
                assert!(provider.args.is_empty());
                let result = check(&engine, &provider, None).await;
                assert!(result.ok, "{name}: {}", result.message);
                let mut cmd = command(name).unwrap();
                cmd.arg("--echo-args")
                    .args(literal)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true);
                let output = spawn(&mut cmd).unwrap().wait_with_output().await.unwrap();
                assert!(output.status.success());
                assert_eq!(
                    serde_json::from_slice::<Value>(&output.stdout).unwrap(),
                    json!(literal)
                );
            }
            // Native .exe configuration continues to work as well.
            let output = command("node.exe")
                .unwrap()
                .args(["--eval", "process.stdout.write('native-ok')"])
                .output()
                .await
                .unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, b"native-ok");
            let events: Vec<Value> = std::fs::read_to_string(std::env::var_os(RECORD).unwrap())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(
                events,
                vec![
                    json!(["app-server"]),
                    json!("initialize"),
                    json!("initialized"),
                    json!("account/read"),
                    json!(["auth", "status", "--json"])
                ]
            );
            let snapshot = engine.store.snapshot().unwrap();
            assert!(snapshot.runs.is_empty() && snapshot.providers.is_empty());
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let npm = dir.path().join("npm 한글 경로");
        std::fs::create_dir_all(&npm).unwrap();
        let output = command("node")
            .unwrap()
            .args(["-p", "process.execPath"])
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        let node = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
        for (name, package, entry) in [
            ("codex", "@openai/codex", "bin/codex.js"),
            ("claude", "@anthropic-ai/claude-code", "cli.js"),
        ] {
            let entry_path = npm.join("node_modules").join(package).join(entry);
            std::fs::create_dir_all(entry_path.parent().unwrap()).unwrap();
            std::fs::write(
                entry_path,
                include_str!("../../tests/fixtures/windows-cli.cjs"),
            )
            .unwrap();
            let shim = include_str!("../../tests/fixtures/npm-shim.cmd")
                .replace("PACKAGE", &package.replace('/', "\\"))
                .replace("ENTRY", &entry.replace('/', "\\"));
            std::fs::write(npm.join(format!("{name}.cmd")), shim.replace('\n', "\r\n")).unwrap();
            // npm also creates a POSIX wrapper. Windows must skip that file.
            std::fs::write(npm.join(name), "#!/bin/sh\nexit 99\n").unwrap();
        }
        let path = std::env::join_paths([npm.as_path(), node.parent().unwrap()]).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "providers::launch::tests::windows_default_templates_launch_npm_clis_without_a_shell", "--nocapture"])
            .env("PATH", path).env(RECORD, dir.path().join("record.jsonl"))
            .stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(30), child.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
