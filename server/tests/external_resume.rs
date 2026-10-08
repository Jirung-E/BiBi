use bibi_core::*;
use bibi_server::{
    config::ServiceConfig,
    providers::{claude_history, codex, external_resume},
    runtime::Engine,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, time::Duration};

struct Fixture {
    engine: Engine,
    dir: tempfile::TempDir,
    root: PathBuf,
    native: String,
    source: PathBuf,
}
impl Fixture {
    async fn new(provider: Provider) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("claude-config");
        fs::create_dir_all(root.join("projects").join("fixture")).unwrap();
        fs::create_dir_all(root.join("sessions")).unwrap();
        let native = uuid::Uuid::new_v4().to_string();
        let store = Store::open(dir.path().join("bibi.sqlite3")).unwrap();
        store
            .add_project(Project {
                id: "p".into(),
                name: "fixture".into(),
                workspace: dir.path().to_string_lossy().into(),
                guild_path: None,
                constraints: vec![],
            })
            .unwrap();
        let source = if provider == Provider::Claude {
            root.join("projects")
                .join("fixture")
                .join(format!("{native}.jsonl"))
        } else {
            dir.path().join("codex-history.json")
        };
        if provider == Provider::Claude {
            let row = json!({"type":"user","uuid":"original-user","sessionId":native,"cwd":dir.path(),"message":{"role":"user","content":"ORIGINAL_CONTEXT"},"timestamp":"2026-10-01T00:00:00Z"});
            fs::write(&source, format!("{row}\n")).unwrap();
            fs::write(
                dir.path().join(format!("{native}.fixture.json")),
                r#"["ORIGINAL_CONTEXT"]"#,
            )
            .unwrap();
        } else {
            fs::write(&source, json!({"native":native,"cwd":dir.path(),"turns":[{"id":"original","status":"completed","startedAt":1,"items":[{"type":"userMessage","id":"original-user","content":[{"type":"text","text":"ORIGINAL_CONTEXT"}]}]}]}).to_string()).unwrap();
        }
        let script = if provider == Provider::Claude {
            "claude.mjs"
        } else {
            "codex-resume.mjs"
        };
        let mut args = vec![format!(
            "{}/tests/fixtures/{script}",
            env!("CARGO_MANIFEST_DIR")
        )];
        if provider == Provider::Codex {
            args.push(source.to_string_lossy().into());
        }
        store
            .save_provider(
                ProviderConfig {
                    id: "provider".into(),
                    host_id: "local".into(),
                    remote_id: None,
                    name: "fixture".into(),
                    adapter: provider,
                    command: "node".into(),
                    args,
                    endpoint: String::new(),
                    models: vec![],
                    api_key_set: false,
                    quota_source: None,
                    ollama: None,
                },
                None,
            )
            .unwrap();
        let engine = Engine::new(store, ServiceConfig::new(dir.path().into()));
        Self {
            engine,
            dir,
            root,
            native,
            source,
        }
    }
    async fn import(&self, provider: &Provider) -> Run {
        let engine = self.engine.configured("provider").unwrap();
        if *provider == Provider::Claude {
            claude_history::discover_in(&engine, "p", &self.root).unwrap();
        } else {
            codex::discover(&engine, "p").await.unwrap();
        }
        self.engine
            .store
            .snapshot()
            .unwrap()
            .runs
            .into_iter()
            .find(|r| r.session_key.as_deref() == Some(&self.native))
            .unwrap()
    }
    fn registration(&self, pid: u32) {
        // pid=0 is confirmed dead. A live process without a start identity is
        // intentionally unverifiable, never treated as safely stopped.
        fs::write(self.root.join("sessions").join("external.json"),json!({"pid":pid,"sessionId":self.native,"cwd":self.dir.path(),"startedAt":1,"status":"idle"}).to_string()).unwrap();
    }
    fn patch(&self, key: &str, value: Value) {
        let mut state: Value = serde_json::from_slice(&fs::read(&self.source).unwrap()).unwrap();
        state[key] = value;
        fs::write(&self.source, state.to_string()).unwrap();
    }
}
fn follow(engine: &Engine, run: &Run, id: &str) -> Receipt {
    engine
        .store
        .submit(Submission {
            attachments: vec![],
            submission_id: id.into(),
            project_key: run.project_key.clone(),
            work_id: Some(run.work_id.clone()),
            title: None,
            question: id.into(),
            provider: run.provider.clone(),
            provider_id: run.provider_id.clone(),
            model: run.model.clone(),
            host_id: run.host_id.clone(),
            role: run.role.clone(),
            mode: SubmitMode::Continue,
            target_run_id: Some(run.id.clone()),
            expected_turn_id: run.turn_id.clone(),
            expected_context_revision: Some(run.context_revision),
            read_only: run.read_only,
            approval_mode: None,
        })
        .unwrap()
}
async fn finish(engine: &Engine, receipt: &Receipt) -> RunDetail {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let detail = engine.store.detail(&receipt.run_id).unwrap();
            if detail.run.state.terminal() {
                return detail;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("mock provider timed out")
}
async fn continuity(provider: Provider) {
    let mut f = Fixture::new(provider.clone()).await;
    let imported = f.import(&provider).await;
    f.engine
        .store
        .rename_session(&imported.id, "kept title")
        .unwrap();
    let imported = f.engine.store.run(&imported.id).unwrap();
    let mut group = imported.clone();
    group.id = "other-group-run".into();
    group.session_id = group.id.clone();
    group.work_id = "other-group".into();
    group.session_key = Some(uuid::Uuid::new_v4().to_string());
    group.context.work_id = group.work_id.clone();
    f.engine
        .store
        .import_external(group.clone(), vec![])
        .unwrap();
    let groups = f
        .engine
        .store
        .set_session_groups(
            &imported.id,
            vec![imported.work_id.clone(), group.work_id],
            vec![imported.work_id.clone()],
        )
        .unwrap();
    let confirmation = external_resume::check(&f.engine, &imported.id)
        .await
        .unwrap();
    assert_eq!(confirmation["requires_confirmation"], true);
    assert!(
        external_resume::adopt(&f.engine, &imported.id, &f.native, false)
            .await
            .is_err()
    );
    let resumed = external_resume::adopt(&f.engine, &imported.id, &f.native, true)
        .await
        .unwrap();
    assert_eq!(resumed.id, imported.id);
    assert_eq!(resumed.session_id(), imported.session_id());
    assert_eq!(resumed.work_id, imported.work_id);
    assert_eq!(resumed.conversation_id, imported.conversation_id);
    assert_eq!(resumed.title, "kept title");
    assert_eq!(resumed.origin, Origin::Managed);
    assert_eq!(resumed.session_key.as_deref(), Some(f.native.as_str()));
    assert!(resumed.capabilities.continue_session.supported);
    assert_eq!(
        external_resume::adopt(&f.engine, &imported.id, &f.native, true)
            .await
            .unwrap()
            .id,
        resumed.id
    );
    // The transfer creates no request or model prompt by itself.
    assert_eq!(f.engine.store.snapshot().unwrap().runs.len(), 2);
    assert_eq!(
        f.engine.store.detail(&resumed.id).unwrap().messages[0].text,
        "ORIGINAL_CONTEXT"
    );
    f.engine.start().await.unwrap();
    let first = finish(&f.engine, &follow(&f.engine, &resumed, "FIRST_BIBI")).await;
    assert_eq!(
        first.run.state,
        RunState::Completed,
        "{:?}",
        first.run.error
    );
    let answer: Value = serde_json::from_str(
        &first
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(answer["turns"], 2);
    assert_eq!(answer["first"], "ORIGINAL_CONTEXT");
    assert_eq!(answer["resumed"], true);
    assert_eq!(first.run.session_key, resumed.session_key);
    assert_eq!(first.run.session_id(), resumed.session_id());
    let second = finish(&f.engine, &follow(&f.engine, &first.run, "SECOND_BIBI")).await;
    assert_eq!(
        second.run.state,
        RunState::Completed,
        "{:?}",
        second.run.error
    );
    // Stale observations and explicit re-imports cannot overwrite managed state.
    f.engine
        .store
        .import_external(imported.clone(), vec![])
        .unwrap();
    f.import(&provider).await;
    assert_eq!(
        f.engine.store.run(&resumed.id).unwrap().origin,
        Origin::Managed
    );
    assert_eq!(
        f.engine.store.snapshot().unwrap().session_groups,
        vec![groups.clone()]
    );
    let before = f
        .engine
        .store
        .detail(&second.run.id)
        .unwrap()
        .conversation
        .len();
    f.engine.stop().await;
    let store = Store::open(f.dir.path().join("bibi.sqlite3")).unwrap();
    f.engine = Engine::new(store, ServiceConfig::new(f.dir.path().into()));
    f.engine.start().await.unwrap();
    let third = finish(&f.engine, &follow(&f.engine, &second.run, "AFTER_RESTART")).await;
    assert_eq!(
        third.run.state,
        RunState::Completed,
        "{:?}",
        third.run.error
    );
    assert_eq!(third.run.session_key, resumed.session_key);
    assert_eq!(third.run.session_id(), resumed.session_id());
    assert!(third.conversation.len() > before);
    assert_eq!(
        f.engine.store.snapshot().unwrap().session_groups,
        vec![groups]
    );
    let answer: Value = serde_json::from_str(
        &third
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(answer["turns"], 4);
    assert_eq!(answer["first"], "ORIGINAL_CONTEXT");
    f.engine.stop().await;
}
#[tokio::test]
async fn claude_external_resume_keeps_native_context_and_identity_after_restart() {
    continuity(Provider::Claude).await;
}
#[tokio::test]
async fn codex_external_resume_keeps_native_context_and_identity_after_restart() {
    continuity(Provider::Codex).await;
}
#[tokio::test]
async fn claude_dead_owner_is_ready_but_unknown_process_cannot_be_overridden() {
    let f = Fixture::new(Provider::Claude).await;
    f.registration(0);
    let run = f.import(&Provider::Claude).await;
    assert_eq!(
        external_resume::check(&f.engine, &run.id).await.unwrap()["requires_confirmation"],
        false
    );
    f.registration(std::process::id());
    assert!(
        external_resume::adopt(&f.engine, &run.id, &f.native, true)
            .await
            .is_err()
    );
    assert_eq!(
        f.engine.store.run(&run.id).unwrap().origin,
        Origin::External
    );
    f.registration(0);
    assert!(
        external_resume::adopt(&f.engine, &run.id, &f.native, false)
            .await
            .is_ok()
    );
}
#[tokio::test]
async fn codex_active_missing_or_wrong_thread_never_transfers_or_sends_a_prompt() {
    for (key, value) in [
        ("busy", json!(true)),
        ("missing", json!(true)),
        ("wrong_id", json!(true)),
        ("cwd", json!("/other-workspace")),
    ] {
        let f = Fixture::new(Provider::Codex).await;
        let run = f.import(&Provider::Codex).await;
        f.patch(key, value);
        assert!(
            external_resume::adopt(&f.engine, &run.id, &f.native, true)
                .await
                .is_err(),
            "{key}"
        );
        assert_eq!(
            f.engine.store.run(&run.id).unwrap().origin,
            Origin::External
        );
        let calls = fs::read_to_string(f.source.with_extension("json.calls")).unwrap();
        assert!(!calls.contains("turn/start"));
        assert!(!calls.contains("thread/resume"));
        assert!(!calls.contains("thread/start"));
    }
}
#[tokio::test]
async fn claude_source_missing_wrong_workspace_and_subagent_are_rejected() {
    let f = Fixture::new(Provider::Claude).await;
    let mut run = f.import(&Provider::Claude).await;
    let original = fs::read(&f.source).unwrap();
    fs::remove_file(&f.source).unwrap();
    assert!(
        external_resume::adopt(&f.engine, &run.id, &f.native, true)
            .await
            .is_err()
    );
    fs::write(&f.source, original).unwrap();
    run.workspace = "/different-workspace".into();
    f.engine.store.import_external(run.clone(), vec![]).unwrap();
    assert!(
        external_resume::adopt(&f.engine, &run.id, &f.native, true)
            .await
            .is_err()
    );
    run.workspace = f.dir.path().to_string_lossy().into();
    run.agent_kind = "subagent".into();
    f.engine.store.import_external(run.clone(), vec![]).unwrap();
    assert!(
        external_resume::adopt(&f.engine, &run.id, &f.native, true)
            .await
            .is_err()
    );
    assert_eq!(
        f.engine.store.run(&run.id).unwrap().origin,
        Origin::External
    );
}
#[tokio::test]
async fn owner_reopened_after_transfer_blocks_first_question_without_new_session() {
    let f = Fixture::new(Provider::Codex).await;
    let run = f.import(&Provider::Codex).await;
    let resumed = external_resume::adopt(&f.engine, &run.id, &f.native, true)
        .await
        .unwrap();
    f.patch("busy", json!(true));
    f.engine.start().await.unwrap();
    let first = finish(&f.engine, &follow(&f.engine, &resumed, "BLOCKED_QUESTION")).await;
    assert_eq!(first.run.state, RunState::Interrupted);
    assert!(first.run.error.as_ref().unwrap().contains("아직 실행 중"));
    assert_eq!(first.run.session_key, resumed.session_key);
    let calls = fs::read_to_string(f.source.with_extension("json.calls")).unwrap();
    assert!(!calls.contains("turn/start"));
    f.patch("busy", json!(false));
    let retry = finish(&f.engine, &follow(&f.engine, &first.run, "RETRY_QUESTION")).await;
    assert_eq!(
        retry.run.state,
        RunState::Completed,
        "{:?}",
        retry.run.error
    );
    assert_eq!(retry.run.session_key, resumed.session_key);
    f.engine.stop().await;
}

#[tokio::test]
async fn transfer_rejects_hidden_stale_and_duplicate_ownership_without_changing_history() {
    let f = Fixture::new(Provider::Claude).await;
    let original = f.import(&Provider::Claude).await;
    f.engine
        .store
        .set_session_hidden(&original.id, true)
        .unwrap();
    assert!(
        external_resume::adopt(&f.engine, &original.id, &f.native, true)
            .await
            .is_err()
    );
    f.engine
        .store
        .set_session_hidden(&original.id, false)
        .unwrap();
    f.engine
        .store
        .rename_session(&original.id, "newer title")
        .unwrap();
    assert!(
        f.engine
            .store
            .adopt_external(&original, vec![], json!({}))
            .is_err()
    );
    assert_eq!(
        f.engine.store.detail(&original.id).unwrap().messages[0].text,
        "ORIGINAL_CONTEXT"
    );
    let mut alias = f.engine.store.run(&original.id).unwrap();
    alias.id = "second-import".into();
    alias.session_id = alias.id.clone();
    f.engine
        .store
        .import_external(alias.clone(), vec![])
        .unwrap();
    let resumed = external_resume::adopt(&f.engine, &original.id, &f.native, true)
        .await
        .unwrap();
    assert_eq!(resumed.title, "newer title");
    assert!(
        external_resume::adopt(&f.engine, &alias.id, &f.native, true)
            .await
            .is_err()
    );
    f.engine
        .store
        .import_external(alias.clone(), vec![])
        .unwrap();
    assert_eq!(
        f.engine.store.run(&alias.id).unwrap().state,
        RunState::Interrupted
    );
}
#[tokio::test]
async fn conflicting_claude_launch_flags_never_transfer_a_conversation() {
    let f = Fixture::new(Provider::Claude).await;
    let run = f.import(&Provider::Claude).await;
    let mut provider = f.engine.store.provider("provider").unwrap();
    provider.args.push("--fork-session".into());
    f.engine.store.save_provider(provider, None).unwrap();
    let error = external_resume::adopt(&f.engine, &run.id, &f.native, true)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("--fork-session"));
    assert_eq!(
        f.engine.store.run(&run.id).unwrap().origin,
        Origin::External
    );
}
#[tokio::test]
async fn wrong_native_resume_response_never_falls_back_to_a_new_thread_or_sends_question() {
    let f = Fixture::new(Provider::Codex).await;
    let run = f.import(&Provider::Codex).await;
    let resumed = external_resume::adopt(&f.engine, &run.id, &f.native, true)
        .await
        .unwrap();
    f.patch("fork", json!(true));
    f.engine.start().await.unwrap();
    let reply = finish(&f.engine, &follow(&f.engine, &resumed, "MUST_NOT_SEND")).await;
    assert_ne!(reply.run.state, RunState::Completed);
    assert_eq!(reply.run.session_key, resumed.session_key);
    assert!(reply.run.error.unwrap().contains("다른 세션"));
    let calls = fs::read_to_string(f.source.with_extension("json.calls")).unwrap();
    assert!(calls.contains("thread/resume"));
    assert!(!calls.contains("thread/start"));
    assert!(!calls.contains("turn/start"));
    assert_eq!(
        f.engine
            .store
            .setting::<Value>(&format!("external_resume:{}", resumed.session_id()))
            .unwrap()
            .unwrap()["pending"],
        true
    );
    f.engine.stop().await;
}
