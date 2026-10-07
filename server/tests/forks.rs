use bibi_core::*;
use bibi_server::{config::ServiceConfig, providers::forks, runtime::Engine};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, time::Duration};
struct Fixture {
    dir: tempfile::TempDir,
    engine: Engine,
    source: Run,
    file: PathBuf,
    native: String,
}
impl Fixture {
    fn new(provider: Provider) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("db")).unwrap();
        store
            .add_project(Project {
                id: "p".into(),
                name: "fixture".into(),
                workspace: dir.path().to_string_lossy().into(),
                guild_path: None,
                constraints: vec![],
            })
            .unwrap();
        store
            .upsert_host(Host {
                id: "local".into(),
                name: "fixture".into(),
                platform: "test".into(),
                kind: "local".into(),
                connected: true,
                observed_at: now(),
                providers: vec![provider.clone()],
                error: None,
            })
            .unwrap();
        let native = uuid::Uuid::new_v4().to_string();
        let root = dir.path().join("claude");
        fs::create_dir_all(root.join("projects/fixture")).unwrap();
        let file = if provider == Provider::Claude {
            root.join("projects/fixture")
                .join(format!("{native}.jsonl"))
        } else {
            dir.path().join("native.json")
        };
        let script = if provider == Provider::Claude {
            "fork-claude.mjs"
        } else {
            "fork-codex.mjs"
        };
        let mut args = vec![format!(
            "{}/tests/fixtures/{script}",
            env!("CARGO_MANIFEST_DIR")
        )];
        if provider == Provider::Codex {
            args.push(file.to_string_lossy().into());
        }
        store.save_provider(serde_json::from_value(json!({"id":"provider","name":"fixture","adapter":provider,"command":"node","args":args,"endpoint":"http://127.0.0.1:1"})).unwrap(),None).unwrap();
        let request:Submission=serde_json::from_value(json!({"submission_id":"original","project_key":"p","question":"FIRST","provider":provider,"provider_id":"provider","model":"fixture-model","host_id":"local","role":"coordinator","mode":"fresh","read_only":true})).unwrap();
        let receipt = store.submit(request).unwrap();
        store.claim_next("local").unwrap();
        store
            .delivered(&receipt.run_id, &native, "original-turn")
            .unwrap();
        store
            .append_output(
                &receipt.run_id,
                "original-answer",
                "assistant",
                "FIRST ANSWER",
            )
            .unwrap();
        store
            .complete(&receipt.run_id, "FIRST ANSWER", UsageStats::default())
            .unwrap();
        store
            .runtime_metadata(
                &receipt.run_id,
                None,
                None,
                Some(file.to_string_lossy().into()),
            )
            .unwrap();
        if provider == Provider::Claude {
            let mut rows = vec![];
            let mut previous = Value::Null;
            for (role, text) in [
                ("user", "FIRST"),
                ("assistant", "FIRST ANSWER"),
                ("user", "FUTURE"),
                ("assistant", "FUTURE ANSWER"),
            ] {
                let id = uuid::Uuid::new_v4().to_string();
                rows.push(json!({"type":role,"uuid":id,"parentUuid":previous,"sessionId":native,"cwd":dir.path(),"message":{"role":role,"content":[{"type":"text","text":text}]},"timestamp":"2026-10-01T00:00:00Z"}));
                previous = json!(id);
            }
            fs::write(
                &file,
                rows.iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n",
            )
            .unwrap();
        } else {
            let turns=[("first","FIRST"),("future","FUTURE")].into_iter().map(|(id,text)|json!({"id":id,"status":"completed","startedAt":1,"items":[{"type":"userMessage","id":format!("{id}-u"),"content":[{"type":"text","text":text}]},{"type":"agentMessage","id":format!("{id}-a"),"text":format!("{text} ANSWER"),"phase":"final_answer"}]})).collect::<Vec<_>>();
            fs::write(&file,json!({"threads":{native.clone():{"id":native,"cwd":dir.path(),"path":file,"model":"fixture-model","turns":turns}}}).to_string()).unwrap();
        }
        let source = store.run(&receipt.run_id).unwrap();
        Self {
            engine: Engine::new(store, ServiceConfig::new(dir.path().into())),
            dir,
            source,
            file,
            native,
        }
    }
    async fn request(&self) -> ForkRequest {
        let points = forks::points(&self.engine, &self.source.id).await.unwrap();
        assert_eq!(points["points"].as_array().unwrap().len(), 2);
        let first = &points["points"][0];
        ForkRequest {
            request_id: "fork-request".into(),
            run_id: self.source.id.clone(),
            point_id: first["id"].as_str().unwrap().into(),
            revision: first["revision"].as_str().unwrap().into(),
            title: "independent branch".into(),
        }
    }
}
fn follow(engine: &Engine, run: &Run) -> Receipt {
    engine.store.submit(serde_json::from_value(json!({"submission_id":id("question"),"project_key":run.project_key,"work_id":run.work_id,"question":"FOLLOWUP","provider":run.provider,"provider_id":run.provider_id,"model":run.model,"host_id":"local","role":run.role,"mode":"continue","target_run_id":run.id,"expected_turn_id":run.turn_id,"expected_context_revision":run.context_revision,"read_only":run.read_only})).unwrap()).unwrap()
}
async fn continuity(provider: Provider) {
    let f = Fixture::new(provider.clone());
    let before = serde_json::to_value(f.engine.store.detail(&f.source.id).unwrap()).unwrap();
    let original = fs::read(&f.file).unwrap();
    let request = f.request().await;
    let (a, b) = tokio::join!(
        forks::fork(&f.engine, &request),
        forks::fork(&f.engine, &request)
    );
    let fork = a.unwrap();
    assert_eq!(fork.id, b.unwrap().id);
    assert_ne!(fork.session_id(), f.source.session_id());
    assert_ne!(fork.session_key, f.source.session_key);
    assert_eq!(fork.parent_session_id, None);
    assert_eq!(
        fork.runtime.fork.as_ref().unwrap().session_id,
        f.source.session_id()
    );
    assert_eq!(fork.state, RunState::Completed);
    let detail = f.engine.store.detail(&fork.id).unwrap();
    let text = serde_json::to_string(&detail.messages).unwrap();
    assert!(text.contains("FIRST"));
    assert!(!text.contains("FUTURE"));
    assert!(detail.approvals.is_empty());
    assert_eq!(f.engine.store.snapshot().unwrap().runs.len(), 2);
    assert_eq!(
        serde_json::to_value(f.engine.store.detail(&f.source.id).unwrap()).unwrap(),
        before
    );
    if provider == Provider::Claude {
        assert_eq!(fs::read(&f.file).unwrap(), original);
        assert!(!f.dir.path().join("claude/questions").exists());
        let copied = fs::read_to_string(fork.runtime.session_file.as_ref().unwrap()).unwrap();
        assert!(!copied.contains("FUTURE"));
        let rows: Vec<Value> = copied
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_ne!(rows[0]["uuid"], rows[0]["forkedFrom"]["messageUuid"]);
        assert_eq!(rows[1]["parentUuid"], rows[0]["uuid"]);
    } else {
        let calls = fs::read_to_string(f.file.with_extension("json.calls")).unwrap();
        assert_eq!(calls.matches("thread/fork").count(), 1);
        assert!(!calls.contains("turn/start"));
    }
    // Reopen the DB and provider connection before the first question on the fork.
    let restored = Engine::new(
        Store::open(f.dir.path().join("db")).unwrap(),
        f.engine.config.clone(),
    );
    restored.start().await.unwrap();
    let receipt = follow(&restored, &fork);
    let detail = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let d = restored.store.detail(&receipt.run_id).unwrap();
            if d.run.state.terminal() {
                break d;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    restored.stop().await;
    assert_eq!(
        detail.run.state,
        RunState::Completed,
        "{:?}",
        detail.run.error
    );
    assert_eq!(detail.run.session_key, fork.session_key);
    assert_eq!(detail.run.session_id(), fork.session_id());
    let reply = detail
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .unwrap();
    assert!(reply.text.contains("FIRST"));
    assert!(!reply.text.contains("FUTURE"));
    assert!(reply.text.contains(fork.session_key.as_ref().unwrap()));
    let after = serde_json::to_value(restored.store.detail(&f.source.id).unwrap()).unwrap();
    // The work inbox is shared by group members; the source session itself is unchanged.
    for key in [
        "run",
        "messages",
        "conversation",
        "children",
        "approvals",
        "inputs",
    ] {
        assert_eq!(after[key], before[key], "source {key} changed");
    }
    assert_eq!(after["inbox"][0], before["inbox"][0]);
    assert_eq!(forks::fork(&restored, &request).await.unwrap().id, fork.id);
    let mut altered = request.clone();
    altered.title = "different".into();
    assert!(forks::fork(&restored, &altered).await.is_err());
    assert_ne!(fork.session_key.as_deref(), Some(f.native.as_str()));
}
#[tokio::test]
async fn codex_fork_keeps_cutoff_new_identity_and_restart_continuity() {
    continuity(Provider::Codex).await;
}
#[tokio::test]
async fn claude_fork_keeps_cutoff_new_identity_and_restart_continuity() {
    continuity(Provider::Claude).await;
}
#[tokio::test]
async fn stale_point_is_rejected_before_creating_native_session() {
    let f = Fixture::new(Provider::Claude);
    let request = f.request().await;
    let text = fs::read_to_string(&f.file)
        .unwrap()
        .replace("FIRST ANSWER", "CHANGED ANSWER");
    fs::write(&f.file, text).unwrap();
    assert!(
        forks::fork(&f.engine, &request)
            .await
            .unwrap_err()
            .to_string()
            .contains("변경")
    );
    assert_eq!(f.engine.store.snapshot().unwrap().runs.len(), 1);
}
#[tokio::test]
async fn codex_unsupported_or_ignored_cutoff_never_publishes_a_fake_fork() {
    for flag in ["unsupported", "ignoreCutoff"] {
        let f = Fixture::new(Provider::Codex);
        let request = f.request().await;
        let mut data: Value = serde_json::from_slice(&fs::read(&f.file).unwrap()).unwrap();
        data[flag] = json!(true);
        fs::write(&f.file, data.to_string()).unwrap();
        assert!(forks::fork(&f.engine, &request).await.is_err());
        assert!(forks::fork(&f.engine, &request).await.is_err());
        assert_eq!(f.engine.store.snapshot().unwrap().runs.len(), 1);
        let calls = fs::read_to_string(f.file.with_extension("json.calls")).unwrap();
        assert_eq!(calls.matches("thread/fork").count(), 1);
        assert!(!calls.contains("turn/start"));
    }
}

#[tokio::test]
async fn fork_copies_memberships_but_not_live_approvals_or_parent_ownership() {
    let f = Fixture::new(Provider::Claude);
    let mut other = f.source.clone();
    other.id = "external-other".into();
    other.session_id = other.id.clone();
    other.work_id = "other-group".into();
    other.origin = Origin::External;
    other.session_key = Some(uuid::Uuid::new_v4().to_string());
    f.engine
        .store
        .import_external(other.clone(), vec![])
        .unwrap();
    let groups = f
        .engine
        .store
        .set_session_groups(
            &f.source.id,
            vec![f.source.work_id.clone(), other.work_id.clone()],
            vec![f.source.work_id.clone()],
        )
        .unwrap();
    f.engine
        .store
        .add_approval(Approval {
            id: "original-approval".into(),
            run_id: f.source.id.clone(),
            native_id: json!("native-approval"),
            kind: "fixture".into(),
            title: "original only".into(),
            detail: json!({}),
            state: "pending".into(),
            created_at: now(),
        })
        .unwrap();
    let request = f.request().await;
    let fork = forks::fork(&f.engine, &request).await.unwrap();
    assert_eq!(fork.read_only, f.source.read_only);
    assert_eq!(fork.approval_mode, f.source.approval_mode);
    assert_eq!(fork.parent_session_id, None);
    assert!(
        f.engine
            .store
            .detail(&fork.id)
            .unwrap()
            .approvals
            .is_empty()
    );
    assert_eq!(
        f.engine.store.approval("original-approval").unwrap().state,
        "pending"
    );
    let snapshot = f.engine.store.snapshot().unwrap();
    assert_eq!(
        snapshot
            .session_groups
            .iter()
            .find(|g| g.session_id == fork.session_id())
            .unwrap()
            .work_ids,
        groups.work_ids
    );
    f.engine
        .store
        .set_session_groups(
            &f.source.id,
            vec![f.source.work_id.clone()],
            groups.work_ids.clone(),
        )
        .unwrap();
    let reopened = Store::open(f.dir.path().join("db")).unwrap();
    assert_eq!(
        reopened
            .snapshot()
            .unwrap()
            .session_groups
            .iter()
            .find(|g| g.session_id == fork.session_id())
            .unwrap()
            .work_ids,
        groups.work_ids
    );
}
#[tokio::test]
async fn openai_history_fork_excludes_later_messages_and_private_runtime_state() {
    let f = Fixture::new(Provider::OpenAi);
    f.engine
        .store
        .add_message(&f.source.id, "user", "FUTURE")
        .unwrap();
    f.engine
        .store
        .add_message(&f.source.id, "assistant", "FUTURE ANSWER")
        .unwrap();
    let request = f.request().await;
    let fork = forks::fork(&f.engine, &request).await.unwrap();
    let receipt = follow(&f.engine, &fork);
    let run = f.engine.store.run(&receipt.run_id).unwrap();
    let history = bibi_server::providers::openai::history(&f.engine, &run).unwrap();
    let text = serde_json::to_string(&history).unwrap();
    assert!(text.contains("FIRST"));
    assert!(!text.contains("FUTURE"));
    assert!(text.contains("FOLLOWUP"));
    assert!(fork.runtime.session_file.is_none());
    assert!(fork.stats.input_tokens.is_none());
}
#[tokio::test]
async fn claude_fork_retains_tool_pairs_and_skips_progress_without_dangling_parents() {
    let f = Fixture::new(Provider::Claude);
    let mut rows: Vec<Value> = fs::read_to_string(&f.file)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let mut tool = rows[1].clone();
    tool["uuid"] = json!(uuid::Uuid::new_v4().to_string());
    tool["message"]["content"] = json!([{"type":"text","text":"tool pending"},{"type":"tool_use","id":"call-1","name":"Read","input":{"file_path":"fixture"}}]);
    let mut progress = tool.clone();
    progress["uuid"] = json!(uuid::Uuid::new_v4().to_string());
    progress["parentUuid"] = tool["uuid"].clone();
    progress["type"] = json!("progress");
    progress.as_object_mut().unwrap().remove("message");
    let mut result = rows[0].clone();
    result["uuid"] = json!(uuid::Uuid::new_v4().to_string());
    result["parentUuid"] = progress["uuid"].clone();
    result["message"]["content"] =
        json!([{"type":"tool_result","tool_use_id":"call-1","content":"READ RESULT"}]);
    rows[1]["parentUuid"] = result["uuid"].clone();
    rows.splice(1..1, [tool, progress, result]);
    fs::write(
        &f.file,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let request = f.request().await;
    let fork = forks::fork(&f.engine, &request).await.unwrap();
    let text = fs::read_to_string(fork.runtime.session_file.unwrap()).unwrap();
    assert!(text.contains("READ RESULT"));
    assert!(!text.contains("FUTURE"));
    let rows: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(!rows.iter().any(|v| v["type"] == "progress"));
    for pair in rows
        .iter()
        .filter(|v| v["uuid"].is_string())
        .collect::<Vec<_>>()
        .windows(2)
    {
        assert_eq!(pair[1]["parentUuid"], pair[0]["uuid"]);
    }
}
