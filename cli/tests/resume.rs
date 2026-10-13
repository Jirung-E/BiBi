use bibi_core::*;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
struct Fixture {
    _dir: tempfile::TempDir,
    data: PathBuf,
    root: PathBuf,
    cwd: PathBuf,
    native: String,
    store: Store,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let root = dir.path().join("profile");
        let cwd = dir.path().join("original 한글 path ' $()");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&data).unwrap();
        let native = uuid::Uuid::new_v4().to_string();
        let parent = root.join("projects").join("original");
        fs::create_dir_all(&parent).unwrap();
        fs::write(parent.join(format!("{native}.jsonl")),format!("{}\n",json!({"type":"user","uuid":"u","sessionId":native,"cwd":cwd,"message":{"content":"find me across folders"}}))).unwrap();
        let store = Store::open(data.join("bibi.sqlite3")).unwrap();
        store
            .save_provider(
                ProviderConfig {
                    id: "claude".into(),
                    name: "work cli".into(),
                    adapter: Provider::Claude,
                    host_id: "local".into(),
                    remote_id: None,
                    command: "claude".into(),
                    args: vec![],
                    endpoint: String::new(),
                    models: vec![],
                    api_key_set: false,
                    quota_source: None,
                    ollama: None,
                },
                None,
            )
            .unwrap();
        Self {
            _dir: dir,
            data,
            root,
            cwd,
            native,
            store,
        }
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_bibi"));
        c.args(["--data-dir"])
            .arg(&self.data)
            .arg("resume")
            .env("CLAUDE_CONFIG_DIR", &self.root)
            .env_remove("BIBI_SERVER")
            .env_remove("BIBI_TOKEN")
            .env_remove("BIBI_TOKEN_FILE")
            .current_dir(self._dir.path())
            .stdin(Stdio::null());
        c
    }
}
#[test]
fn public_cli_lists_from_another_folder_without_import_server_or_desktop() {
    let f = Fixture::new();
    let result = f
        .command()
        .args(["folders", "--list", "--json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["items"].as_array().unwrap().len(), 1);
    assert_eq!(value["items"][0]["native_id"], f.native);
    assert_eq!(value["items"][0]["last_user"], "find me across folders");
    assert!(!f.data.join("service.lock").exists());
    assert!(f.store.snapshot().unwrap().projects.is_empty());
    let no_terminal = f.command().output().unwrap();
    assert!(!no_terminal.status.success());
    assert!(String::from_utf8_lossy(&no_terminal.stderr).contains("선택에는 터미널"));
    let remote = f
        .command()
        .env("BIBI_SERVER", "http://127.0.0.1:1")
        .args(["--list", "--json"])
        .output()
        .unwrap();
    assert!(!remote.status.success());
    assert!(String::from_utf8_lossy(&remote.stderr).contains("실행할 컴퓨터"));
}
#[test]
fn public_cli_runs_native_resume_in_original_directory_and_returns_child_exit_code() {
    let f = Fixture::new();
    let exe = f._dir.path().join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    assert!(
        Command::new("rustc")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../server/tests/fixtures/native-terminal.rs")
            )
            .arg("-o")
            .arg(&exe)
            .status()
            .unwrap()
            .success()
    );
    let report = f._dir.path().join("report.txt");
    let mut p = f.store.provider("claude").unwrap();
    p.command = exe.to_string_lossy().into();
    p.args = vec!["--settings".into(), report.to_string_lossy().into()];
    f.store.save_provider(p, None).unwrap();
    let result = f
        .command()
        .args(["--id", &f.native, "--confirmed-stopped"])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let content = fs::read_to_string(report).unwrap();
    let lines = content.lines().collect::<Vec<_>>();
    assert_eq!(
        fs::canonicalize(lines[0]).unwrap(),
        fs::canonicalize(&f.cwd).unwrap()
    );
    assert_eq!(
        fs::canonicalize(lines[1]).unwrap(),
        fs::canonicalize(&f.root).unwrap()
    );
    assert_eq!(&lines[lines.len() - 2..], &["--resume", &f.native]);
    assert!(f.store.snapshot().unwrap().runs.is_empty());
}
