use bibi_core::*;
use bibi_server::{
    config::ServiceConfig,
    providers::antigravity,
    runtime::{Control, Engine},
};
use serde_json::json;
use std::time::Duration;

fn setup(mode: &str) -> (Engine, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    s.add_project(Project {
        id: "p".into(),
        name: "p".into(),
        workspace: d.path().to_string_lossy().into(),
        guild_path: None,
        constraints: vec![],
    })
    .unwrap();
    s.upsert_host(serde_json::from_value(json!({"id":"local","name":"test","platform":"test","kind":"local","connected":true,"observed_at":now(),"providers":["command"]})).unwrap()).unwrap();
    s.save_provider(serde_json::from_value(json!({"id":"agy","name":"AGY fixture","adapter":"command","command":"node","args":[format!("{}/tests/fixtures/antigravity.mjs",env!("CARGO_MANIFEST_DIR")),d.path().join("args.jsonl"),mode]})).unwrap(),None).unwrap();
    let e = Engine::new(s, ServiceConfig::new(d.path().into()))
        .configured("agy")
        .unwrap();
    (e, d)
}
fn request(key: &str) -> Submission {
    serde_json::from_value(json!({"submission_id":key,"project_key":"p","question":"한글 {model} 질문","provider":"command","provider_id":"agy","model":"fixture","host_id":"local","role":"coordinator","mode":"fresh","read_only":false})).unwrap()
}
#[test]
fn recognizes_native_agy_without_matching_unrelated_commands() {
    for command in ["agy", "AGY.EXE", "C:\\Programs\\agy.exe", "/opt/bin/agy"] {
        assert!(antigravity::matches(command));
    }
    for command in ["agy.cmd", "legacy-agy", "agy.exe.bat", "node"] {
        assert!(!antigravity::matches(command));
    }
}
#[tokio::test]
async fn headless_uses_exact_native_conversation_and_does_not_repeat_final_text() {
    let (e, _d) = setup("success");
    let first = e.store.submit(request("first")).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let run = e.store.claim_next("local").unwrap().unwrap();
    antigravity::execute(&e, run, rx).await.unwrap();
    drop(tx);
    let first_run = e.store.run(&first.run_id).unwrap();
    assert_eq!(first_run.state, RunState::Completed);
    assert_eq!(first_run.stats.output_tokens, Some(3));
    assert_eq!(
        e.store
            .detail(&first.run_id)
            .unwrap()
            .messages
            .iter()
            .filter(|m| m.role == "assistant")
            .count(),
        1
    );
    let mut next = request("second");
    next.mode = SubmitMode::Continue;
    next.target_run_id = Some(first_run.id.clone());
    next.expected_turn_id = first_run.turn_id;
    next.expected_context_revision = Some(1);
    let second = e.store.submit(next).unwrap();
    let run = e.store.claim_next("local").unwrap().unwrap();
    let (_tx, rx) = tokio::sync::mpsc::channel(8);
    antigravity::execute(&e, run, rx).await.unwrap();
    let detail = e.store.detail(&second.run_id).unwrap();
    assert_eq!(detail.run.session_key, first_run.session_key);
    assert_eq!(detail.run.session_id, first_run.session_id);
    assert!(detail.messages.iter().any(|m| m.text == "한글 이어서"));
}
#[tokio::test]
async fn ide_launcher_is_rejected_before_sending_a_prompt() {
    let (e, d) = setup("legacy");
    e.store.submit(request("legacy")).unwrap();
    let run = e.store.claim_next("local").unwrap().unwrap();
    let (_tx, rx) = tokio::sync::mpsc::channel(8);
    assert!(
        antigravity::execute(&e, run, rx)
            .await
            .unwrap_err()
            .to_string()
            .contains("IDE")
    );
    let calls = std::fs::read_to_string(d.path().join("args.jsonl")).unwrap();
    assert_eq!(calls.lines().count(), 1);
    assert!(calls.contains("--help"));
    assert!(!calls.contains("한글"));
}
#[tokio::test]
async fn stalled_headless_process_can_be_interrupted() {
    let (e, _d) = setup("hang");
    let receipt = e.store.submit(request("hang")).unwrap();
    let run = e.store.claim_next("local").unwrap().unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let worker = e.clone();
    let task = tokio::spawn(async move { antigravity::execute(&worker, run, rx).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while e.store.run(&receipt.run_id).unwrap().turn_id.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tx.send(Control::Interrupt).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        e.store.run(&receipt.run_id).unwrap().state,
        RunState::Interrupted
    );
}
