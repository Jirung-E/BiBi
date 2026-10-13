use super::*;
use crate::catalog::tests::Fixture;
use std::{fs, process::Command, sync::OnceLock};
fn program() -> PathBuf {
    static FIXTURE: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let d = tempfile::tempdir().unwrap();
            let exe = d.path().join(if cfg!(windows) {
                "native-fixture.exe"
            } else {
                "native-fixture"
            });
            let status = Command::new("rustc")
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-terminal.rs"),
                )
                .args(["-o"])
                .arg(&exe)
                .status()
                .unwrap();
            assert!(status.success());
            (d, exe)
        })
        .1
        .clone()
}
#[tokio::test]
async fn real_process_resumes_exact_id_folder_config_without_prompt_and_ticket_is_single_use() {
    let f = Fixture::new();
    let cwd = f.workspace("working directory ' ; $()");
    let (native, _) = f.claude(&cwd, "history");
    let mut provider = f.engine.store.provider("claude").unwrap();
    provider.command = program().to_string_lossy().into();
    let output = f.dir.path().join("argv.txt");
    provider.args = vec!["--settings".into(), output.to_string_lossy().into()];
    f.engine.store.save_provider(provider, None).unwrap();
    let item = catalog::list(&f.engine, &native, None, None)
        .unwrap()
        .items
        .remove(0);
    assert!(prepare(&f.engine, &item.id, false).await.is_err());
    let result = prepare_local(&f.engine, &item.id, true).await.unwrap();
    let ticket = result["ticket"].as_str().unwrap();
    assert!(
        prepare(&f.engine, &item.id, true)
            .await
            .unwrap_err()
            .to_string()
            .contains("터미널")
    );
    let status = run_ticket(&f.engine.config.data_dir, ticket).await.unwrap();
    assert_eq!(status, 7);
    let lines = fs::read_to_string(output)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert!(claude_live::same_workspace(
        &lines[0],
        &cwd.to_string_lossy()
    ));
    assert!(claude_live::same_workspace(
        &lines[1],
        &f.root.to_string_lossy()
    ));
    assert_eq!(
        &lines[lines.len() - 2..],
        &["--resume".to_string(), native.clone()]
    );
    assert!(
        !lines
            .iter()
            .any(|s| s == "-p" || s == "--print" || s == "history")
    );
    assert!(
        !f.engine
            .store
            .terminal_in_use(&Provider::Claude, &native)
            .unwrap()
    );
    assert!(run_ticket(&f.engine.config.data_dir, ticket).await.is_err());
}
#[tokio::test]
async fn changed_provider_or_transcript_never_launches_stale_ticket() {
    let f = Fixture::new();
    let cwd = f.workspace("A");
    let (native, path) = f.claude(&cwd, "history");
    let item = catalog::list(&f.engine, &native, None, None)
        .unwrap()
        .items
        .remove(0);
    let plan = prepare(&f.engine, &item.id, true).await.unwrap();
    let mut p = f.engine.store.provider("claude").unwrap();
    p.name = "changed".into();
    f.engine.store.save_provider(p, None).unwrap();
    assert!(
        run_ticket(&f.engine.config.data_dir, plan["ticket"].as_str().unwrap())
            .await
            .unwrap_err()
            .to_string()
            .contains("설정이 변경")
    );
    let plan = prepare(&f.engine, &item.id, true).await.unwrap();
    fs::write(path, "{}\n").unwrap();
    assert!(
        run_ticket(&f.engine.config.data_dir, plan["ticket"].as_str().unwrap())
            .await
            .is_err()
    );
    assert!(
        !f.engine
            .store
            .terminal_in_use(&Provider::Claude, &native)
            .unwrap()
    );
}
#[test]
fn positional_prompts_headless_and_identity_overrides_are_rejected() {
    for bad in [
        vec!["-p", "hello"],
        vec!["--resume", "other"],
        vec!["--session-id=x"],
        vec!["--output-format", "json"],
        vec!["untrusted prompt"],
    ] {
        assert!(
            launch_args(
                &Provider::Claude,
                &bad.into_iter().map(str::to_string).collect::<Vec<_>>(),
                None,
                "native"
            )
            .is_err()
        );
    }
    assert!(launch_args(&Provider::Codex, &["exec".into()], None, "native").is_err());
    let args = launch_args(
        &Provider::Codex,
        &[
            "--profile".into(),
            "work".into(),
            "--config".into(),
            "key=value".into(),
        ],
        None,
        "native",
    )
    .unwrap();
    assert_eq!(
        args,
        vec![
            "--profile",
            "work",
            "--config",
            "key=value",
            "resume",
            "native"
        ]
    );
}
#[tokio::test]
async fn running_original_with_unverified_identity_cannot_be_claimed_by_confirmation() {
    let f = Fixture::new();
    let cwd = f.workspace("A");
    let (native, _) = f.claude(&cwd, "history");
    fs::create_dir_all(f.root.join("sessions")).unwrap();
    fs::write(f.root.join("sessions").join(format!("{}.json",std::process::id())),json!({"pid":std::process::id(),"sessionId":native,"cwd":cwd,"startedAt":now(),"status":"working"}).to_string()).unwrap();
    let item = catalog::list(&f.engine, &native, None, None)
        .unwrap()
        .items
        .remove(0);
    assert!(prepare(&f.engine, &item.id, true).await.is_err());
}
#[tokio::test]
async fn local_cli_coordinates_with_existing_owner_without_listener() {
    let f = Fixture::new();
    let cwd = f.workspace("A");
    let (native, _) = f.claude(&cwd, "history");
    let entry = catalog::list(&f.engine, &native, None, None)
        .unwrap()
        .items
        .remove(0);
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(f.engine.config.data_dir.join("service.lock"))
        .unwrap();
    lock.lock().unwrap();
    let worker = tokio::spawn(serve_requests(f.engine.clone()));
    let result = prepare_local(&f.engine, &entry.id, true).await.unwrap();
    assert!(result["ticket"].is_string());
    cancel_ticket(&f.engine.store, result["ticket"].as_str().unwrap());
    f.engine.stop().await;
    worker.await.unwrap();
    assert!(
        !f.engine
            .store
            .terminal_in_use(&Provider::Claude, &native)
            .unwrap()
    );
}
