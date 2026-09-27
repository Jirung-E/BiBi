//! Resolve installed CLIs without a command shell, for probes and sessions.
use anyhow::Result;
use serde::Serialize;
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

#[derive(Debug, Serialize)]
pub struct Launcher {
    pub executable: PathBuf,
    pub entrypoint: Option<PathBuf>,
}

pub(crate) fn describe(program: &str) -> Result<Launcher> {
    plan(program, &search_paths(), cfg!(windows))
}

pub(crate) fn command(program: &str) -> Result<Command> {
    let paths = search_paths();
    let launcher = plan(program, &paths, cfg!(windows))?;
    let mut command = Command::new(launcher.executable);
    if let Some(entry) = launcher.entrypoint {
        command.arg(entry);
    }
    command.env("PATH", std::env::join_paths(paths)?);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    Ok(command)
}

fn plan(program: &str, paths: &[PathBuf], windows: bool) -> Result<Launcher> {
    let requested = Path::new(program.trim());
    if windows
        && (extension_is(requested, "cmd")
            || extension_is(requested, "bat")
            || requested.file_name().is_some_and(|n| {
                n.to_string_lossy().eq_ignore_ascii_case("cmd")
                    || n.to_string_lossy().eq_ignore_ascii_case("cmd.exe")
            }))
    {
        return Err(LaunchError("cmd.exe 및 .cmd/.bat 실행은 지원하지 않습니다. codex 또는 claude 이름이나 네이티브 .exe 경로를 입력하세요.".into()).into());
    }
    if let Some(executable) = resolve(program, paths, windows) {
        return Ok(Launcher {
            executable,
            entrypoint: None,
        });
    }
    if windows && let Some(entry) = npm_entrypoint(program, paths) {
        // Read package metadata, never parse or execute npm's shell wrappers.
        if extension_is(&entry, "exe") || extension_is(&entry, "com") {
            return Ok(Launcher {
                executable: entry,
                entrypoint: None,
            });
        }
        let executable = resolve("node", paths, true).ok_or_else(|| LaunchError(
            "npm CLI에 필요한 node.exe를 찾지 못했습니다. Node.js 설치와 BiBi 서버의 PATH를 확인하세요.".into()))?;
        return Ok(Launcher {
            executable,
            entrypoint: Some(entry),
        });
    }
    Err(LaunchError(format!("실행 파일을 찾지 못했습니다: '{program}'. BiBi 서버 PC의 .exe 설치 경로 또는 npm 패키지를 확인하세요. 실행 파일 항목에 .exe 전체 경로를 지정할 수 있습니다.")).into())
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
            "{reason}{code} 실행 파일: {}",
            command.as_std().get_program().to_string_lossy()
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
    let bases: Vec<_> = if program.is_absolute() || program.components().count() > 1 {
        vec![program.to_path_buf()]
    } else {
        paths.iter().map(|dir| dir.join(program)).collect()
    };
    let files: Vec<_> = if windows && program.extension().is_none() {
        // Search the complete PATH for native binaries before considering npm.
        ["exe", "com"]
            .into_iter()
            .flat_map(|ext| bases.iter().map(move |base| base.with_extension(ext)))
            .collect()
    } else if !windows || extension_is(program, "exe") || extension_is(program, "com") {
        bases
    } else {
        vec![]
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

fn npm_entrypoint(program: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    let program = program.trim();
    let package = match program.to_ascii_lowercase().as_str() {
        "codex" => "@openai/codex",
        "claude" => "@anthropic-ai/claude-code",
        _ => return None,
    };
    for path in paths {
        let root = path.join("node_modules").join(package);
        let manifest = root.join("package.json");
        if manifest.metadata().ok().is_none_or(|m| m.len() > 64 * 1024) {
            continue;
        }
        let Some(value) = std::fs::read(&manifest)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        else {
            continue;
        };
        if value["name"] != package {
            continue;
        }
        let Some(bin) = value["bin"].as_str().or_else(|| {
            value["bin"]
                .get(program.to_ascii_lowercase())
                .and_then(|v| v.as_str())
        }) else {
            continue;
        };
        let candidate = root.join(bin);
        let (Ok(root), Ok(entry)) = (root.canonicalize(), candidate.canonicalize()) else {
            continue;
        };
        if entry.is_file()
            && entry.starts_with(&root)
            && ["js", "cjs", "mjs", "exe", "com"]
                .iter()
                .any(|e| extension_is(&entry, e))
        {
            return Some(entry);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_native_binary_wins_over_earlier_npm_wrappers() {
        let dir = tempfile::tempdir().unwrap();
        let npm = dir.path().join("npm 한글 경로");
        let native = dir.path().join("native");
        std::fs::create_dir_all(&npm).unwrap();
        std::fs::create_dir_all(&native).unwrap();
        std::fs::write(npm.join("codex"), "#!/bin/sh").unwrap();
        std::fs::write(npm.join("codex.cmd"), "@echo obsolete wrapper").unwrap();
        std::fs::write(native.join("codex.exe"), "fixture").unwrap();
        let paths = vec![npm.clone(), native.clone()];
        assert_eq!(
            plan("codex", &paths, true).unwrap().executable,
            native.join("codex.exe")
        );
        assert_eq!(
            resolve(native.join("codex").to_str().unwrap(), &paths, true),
            Some(native.join("codex.exe"))
        );
        assert_eq!(resolve("codex", &paths, false), Some(npm.join("codex")));
        for blocked in ["codex.cmd", "custom.bat", "cmd", "CMD.EXE"] {
            assert!(plan(blocked, &paths, true).is_err());
        }
        assert!(plan("missing", &paths, true).is_err());
    }

    #[test]
    fn npm_installation_uses_manifest_without_requiring_a_batch_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        let node = dir.path().join("node.exe");
        std::fs::write(&node, "fixture").unwrap();
        let paths = vec![dir.path().to_path_buf()];
        for (name, package, bin) in [
            ("codex", "@openai/codex", "bin/codex.js"),
            ("claude", "@anthropic-ai/claude-code", "cli.js"),
        ] {
            let root = dir.path().join("node_modules").join(package);
            let entry = root.join(bin);
            std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
            std::fs::write(&entry, "// fixture").unwrap();
            std::fs::write(
                root.join("package.json"),
                serde_json::json!({"name":package,"bin":{name:bin}}).to_string(),
            )
            .unwrap();
            let launch = plan(name, &paths, true).unwrap();
            assert_eq!(launch.executable, node);
            assert_eq!(launch.entrypoint, Some(entry.canonicalize().unwrap()));
            std::fs::write(
                dir.path().join(format!("{name}.cmd")),
                "@echo different npm version",
            )
            .unwrap();
            assert_eq!(
                plan(name, &paths, true).unwrap().entrypoint,
                launch.entrypoint
            );
            std::fs::write(
                root.join("package.json"),
                serde_json::json!({"name":package,"bin":{name:"../escape.cmd"}}).to_string(),
            )
            .unwrap();
            assert!(plan(name, &paths, true).is_err());
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
            std::fs::write(
                npm.join("node_modules").join(package).join("package.json"),
                json!({"name":package,"bin":{name:entry}}).to_string(),
            )
            .unwrap();
            std::fs::write(
                npm.join(format!("{name}.cmd")),
                "@echo MUST NOT RUN & exit /b 99",
            )
            .unwrap();
            // npm also creates a POSIX wrapper. Windows must skip that file.
            std::fs::write(npm.join(name), "#!/bin/sh\nexit 99\n").unwrap();
        }
        let path = std::env::join_paths([npm.as_path(), node.parent().unwrap()]).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "providers::launch::tests::windows_default_templates_launch_npm_clis_without_a_shell", "--nocapture"])
            .env("PATH", path).env("COMSPEC", dir.path().join("no-command-shell.exe")).env(RECORD, dir.path().join("record.jsonl"))
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
