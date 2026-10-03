use bibi_core::*;
use bibi_server::{
    config::ServiceConfig,
    providers::{claude_history, imports},
    runtime::Engine,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn fixture(adapter: Provider) -> (Engine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().unwrap();
    store
        .upsert_host(Host {
            id: "local".into(),
            name: "test".into(),
            platform: "fixture".into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![adapter.clone()],
            error: None,
        })
        .unwrap();
    store
        .add_project(Project {
            id: "p".into(),
            name: "project".into(),
            workspace: dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    store
        .save_provider(
            ProviderConfig {
                id: "provider".into(),
                host_id: "local".into(),
                remote_id: None,
                name: "provider".into(),
                adapter,
                command: "must-not-run".into(),
                args: vec![],
                endpoint: "http://127.0.0.1:1".into(),
                models: vec![],
                api_key_set: false,
                quota_source: None,
                ollama: None,
            },
            None,
        )
        .unwrap();
    let engine = Engine::new(store, ServiceConfig::new(dir.path().into()))
        .configured("provider")
        .unwrap();
    (engine, dir)
}
fn upload(engine: &Engine, document: Value) -> anyhow::Result<Value> {
    imports::upload(
        engine,
        imports::Upload {
            project_key: "p".into(),
            provider_id: "provider".into(),
            document: document.to_string(),
        },
    )
}
fn transcript() -> Value {
    json!({"title":"이어갈 대화","model":"test-model","messages":[{"role":"user","content":"첫 질문"},{"role":"assistant","content":"이전 답변"},{"role":"system","content":"외부 설정 기록"}]})
}
#[test]
fn file_imports_continue_same_bibi_conversation_without_running_native_programs() {
    for provider in [Provider::Ollama, Provider::OpenAi, Provider::Command] {
        let (engine, _dir) = fixture(provider.clone());
        assert_eq!(upload(&engine, transcript()).unwrap()["imported"], 1);
        let original = engine.store.snapshot().unwrap().runs.remove(0);
        assert_eq!(original.origin, Origin::Managed);
        assert!(original.capabilities.continue_session.supported);
        assert_eq!(
            engine
                .store
                .detail(&original.id)
                .unwrap()
                .conversation
                .len(),
            3
        );
        let receipt = engine
            .store
            .submit(Submission {
                submission_id: "continue".into(),
                project_key: "p".into(),
                work_id: Some(original.work_id.clone()),
                title: None,
                question: "후속 질문".into(),
                provider,
                provider_id: Some("provider".into()),
                model: "test-model".into(),
                host_id: "local".into(),
                role: original.role.clone(),
                mode: SubmitMode::Continue,
                target_run_id: Some(original.id.clone()),
                expected_turn_id: None,
                expected_context_revision: Some(1),
                read_only: original.read_only,
                approval_mode: None,
            })
            .unwrap();
        let next = engine.store.run(&receipt.run_id).unwrap();
        assert_eq!(next.session_id(), original.session_id());
        assert_eq!(next.continued_from.as_deref(), Some(original.id.as_str()));
        let history = bibi_server::providers::openai::history(&engine, &next).unwrap();
        assert!(history.iter().any(|v| v["content"] == "이전 답변"));
        assert!(history.iter().any(|v| {
            v["content"]
                .as_str()
                .unwrap_or("")
                .contains("외부 설정 기록")
        }));
        upload(&engine, transcript()).unwrap();
        assert_eq!(engine.store.snapshot().unwrap().runs.len(), 2);
        assert_eq!(
            engine
                .store
                .detail(&next.id)
                .unwrap()
                .conversation
                .last()
                .unwrap()
                .text,
            "후속 질문"
        );
    }
}
#[test]
fn file_import_validates_the_entire_document_before_any_write() {
    let (engine, _dir) = fixture(Provider::Ollama);
    for document in [
        json!({"messages":[]}),
        json!([{"role":"assistant","content":""}]),
        json!([{"role":"developer","content":"x"}]),
        json!([{"role":"user","content":[{"type":"image"}]}]),
        json!([{"role":"user","content":"x"},{"role":"assistant","content":"", "tool_calls":[{"id":"unresolved"}]}]),
    ] {
        assert!(upload(&engine, document).is_err());
        assert!(engine.store.snapshot().unwrap().runs.is_empty());
    }
    let oversized = imports::Upload {
        project_key: "p".into(),
        provider_id: "provider".into(),
        document: "x".repeat(imports::MAX_FILE_BYTES + 1),
    };
    assert!(imports::upload(&engine, oversized).is_err());
    let (engine, _dir) = fixture(Provider::Claude);
    assert!(upload(&engine, transcript()).is_err());
}
fn row(
    native: &str,
    cwd: &Path,
    id: &str,
    parent: Option<&str>,
    role: &str,
    content: Value,
) -> Value {
    json!({"type":role,"uuid":id,"parentUuid":parent,"sessionId":native,"cwd":cwd,"timestamp":"2026-09-30T12:00:00Z","message":{"model":"claude-test","content":content}})
}
fn write(path: &Path, rows: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
    )
    .unwrap();
}
#[test]
fn claude_import_preserves_branch_identity_children_scope_and_hides_thinking() {
    let (engine, dir) = fixture(Provider::Claude);
    let native = uuid::Uuid::new_v4().to_string();
    let file = dir
        .path()
        .join("projects/encoded-project")
        .join(format!("{native}.jsonl"));
    let rows = vec![
        row(&native, dir.path(), "u", None, "user", json!("질문")),
        row(
            &native,
            dir.path(),
            "old",
            Some("u"),
            "assistant",
            json!([{"type":"text","text":"폐기한 분기"}]),
        ),
        row(
            &native,
            dir.path(),
            "a",
            Some("u"),
            "assistant",
            json!([{"type":"thinking","thinking":"private"},{"type":"text","text":"현재 답변"}]),
        ),
        json!({"type":"custom-title","sessionId":native,"customTitle":"사용자 세션 이름"}),
    ];
    write(&file, &rows);
    let child = file.with_extension("").join("subagents/agent-abc123.jsonl");
    write(
        &child,
        &[
            row(&native, dir.path(), "cu", None, "user", json!("분석")),
            row(
                &native,
                dir.path(),
                "ca",
                Some("cu"),
                "assistant",
                json!([{"type":"text","text":"분석 결과"}]),
            ),
        ],
    );
    let other = uuid::Uuid::new_v4().to_string();
    write(
        &file.with_file_name(format!("{other}.jsonl")),
        &[row(
            &other,
            &dir.path().join("other"),
            "u",
            None,
            "user",
            json!("다른 프로젝트"),
        )],
    );
    let result = claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    assert_eq!(result["imported"], 1);
    assert_eq!(result["errors"], json!([]));
    let snapshot = engine.store.snapshot().unwrap();
    assert_eq!(snapshot.runs.len(), 2);
    let parent = snapshot
        .runs
        .iter()
        .find(|r| r.agent_kind == "session")
        .unwrap();
    assert_eq!(parent.session_key.as_deref(), Some(native.as_str()));
    assert_eq!(parent.title, "사용자 세션 이름");
    assert_eq!(parent.origin, Origin::External);
    assert!(!parent.capabilities.send_to_active.supported);
    assert!(!parent.capabilities.continue_session.supported);
    let detail = engine.store.detail(&parent.id).unwrap();
    assert_eq!(detail.messages.len(), 2);
    assert_eq!(detail.messages[1].text, "현재 답변");
    assert_eq!(detail.children.len(), 1);
    assert_eq!(
        detail.children[0].run.session_key.as_deref(),
        Some("abc123")
    );
    let mut updated = rows.clone();
    updated.push(row(
        &native,
        dir.path(),
        "new",
        Some("u"),
        "assistant",
        json!([{"type":"text","text":"수정한 답변"}]),
    ));
    write(&file, &updated);
    claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    assert_eq!(engine.store.snapshot().unwrap().runs.len(), 2);
    let messages = engine.store.detail(&parent.id).unwrap().messages;
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().any(|m| m.text == "수정한 답변"));
    assert!(!messages.iter().any(|m| m.text == "현재 답변"));
}
#[test]
fn claude_reports_bad_history_and_tolerates_partial_last_line_without_losing_other_sessions() {
    let (engine, dir) = fixture(Provider::Claude);
    let valid = uuid::Uuid::new_v4().to_string();
    let broken = uuid::Uuid::new_v4().to_string();
    for native in [&valid, &broken] {
        let path = dir
            .path()
            .join("projects/project")
            .join(format!("{native}.jsonl"));
        write(
            &path,
            &[row(native, dir.path(), "u", None, "user", json!("질문"))],
        );
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(if native == &valid {
            b"{partial"
        } else {
            b"broken\n"
        });
        fs::write(path, bytes).unwrap();
    }
    let result = claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    assert_eq!(result["imported"], 1);
    assert_eq!(result["errors"][0]["session"], broken);
    assert_eq!(engine.store.snapshot().unwrap().runs.len(), 1);
}

#[tokio::test]
async fn transcript_upload_is_authenticated_bounded_and_works_through_local_api() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (engine, _dir) = fixture(Provider::Ollama);
    let app = bibi_server::router(bibi_server::state(
        engine.store.clone(),
        engine.config.clone(),
        "fixture-token".into(),
    ));
    let body = json!({"project_key":"p","provider_id":"provider","document":json!({"model":"fixture","messages":[{"role":"user","content":"x".repeat(600_000)}]}).to_string()});
    let request = |authorized: bool, body: &Value| {
        let builder = Request::builder()
            .method("POST")
            .uri("/api/import")
            .header("content-type", "application/json");
        let builder = if authorized {
            builder.header("authorization", "Bearer fixture-token")
        } else {
            builder
        };
        builder.body(Body::from(body.to_string())).unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(request(false, &body))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(engine.store.snapshot().unwrap().runs.is_empty());
    assert_eq!(
        app.clone()
            .oneshot(request(true, &body))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(engine.store.snapshot().unwrap().runs.len(), 1);
    let invalid = json!({"project_key":"p","provider_id":"provider","document":"not-json"});
    assert_eq!(
        app.oneshot(request(true, &invalid)).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(engine.store.snapshot().unwrap().runs.len(), 1);
}
#[test]
fn claude_finds_long_first_records_and_does_not_duplicate_managed_sessions() {
    let (engine, dir) = fixture(Provider::Claude);
    let native = uuid::Uuid::new_v4().to_string();
    let file = dir
        .path()
        .join("projects/project")
        .join(format!("{native}.jsonl"));
    write(
        &file,
        &[row(
            &native,
            dir.path(),
            "user",
            None,
            "user",
            json!("x".repeat(200_000)),
        )],
    );
    assert_eq!(
        claude_history::discover_in(&engine, "p", dir.path()).unwrap()["imported"],
        1
    );
    let first = engine.store.snapshot().unwrap().runs.remove(0);
    // A separate BiBi-owned execution with the same native identity suppresses discovery.
    let mut managed = first.clone();
    managed.id = "imported_managed".into();
    managed.session_id = managed.id.clone();
    managed.origin = Origin::Managed;
    engine.store.import_conversation(managed, vec![]).unwrap();
    assert_eq!(
        claude_history::discover_in(&engine, "p", dir.path()).unwrap()["imported"],
        0
    );
    assert_eq!(engine.store.snapshot().unwrap().runs.len(), 2);
}

#[test]
fn claude_accepts_javascript_lone_surrogates_without_changing_pairs_or_literal_escapes() {
    let (engine, dir) = fixture(Provider::Claude);
    let native = uuid::Uuid::new_v4().to_string();
    let file = dir
        .path()
        .join("projects/project")
        .join(format!("{native}.jsonl"));
    let record = row(&native, dir.path(), "user", None, "user", json!("REPLACE"));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let line = record
        .to_string()
        .replace("REPLACE", r"a\ud83d b\ude00 c\ud83d\ude00 d\\ud83d");
    fs::write(&file, format!("{line}\n")).unwrap();
    assert_eq!(
        claude_history::discover_in(&engine, "p", dir.path()).unwrap()["imported"],
        1
    );
    let run = engine.store.snapshot().unwrap().runs.remove(0);
    assert_eq!(
        engine.store.detail(&run.id).unwrap().messages[0].text,
        "a� b� c😀 d\\ud83d"
    );
}

fn current_registration(root: &Path, workspace: &Path, native: &str) -> std::path::PathBuf {
    #[allow(unused_mut)]
    let mut record = json!({"pid":std::process::id(),"sessionId":native,"cwd":workspace,"startedAt":now(),"status":"busy","name":"Active Claude fixture"});
    #[cfg(unix)]
    {
        let output = std::process::Command::new("/bin/ps")
            .args(["-p", &std::process::id().to_string(), "-o", "lstart="])
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .output()
            .unwrap();
        record["procStart"] = json!(String::from_utf8(output.stdout).unwrap().trim());
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::FILETIME,
            System::Threading::{GetCurrentProcess, GetProcessTimes},
        };
        let empty = || FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (empty(), empty(), empty(), empty());
        // This fixture represents this exact live process, not the time the
        // session JSON was written. A missing identity must remain Uncertain.
        assert_ne!(
            unsafe {
                GetProcessTimes(
                    GetCurrentProcess(),
                    &mut created,
                    &mut exited,
                    &mut kernel,
                    &mut user,
                )
            },
            0
        );
        record["procStartFt"] = json!(
            (((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64).to_string()
        );
    }
    fs::create_dir_all(root.join("sessions")).unwrap();
    let path = root.join("sessions").join(format!("{native}.json"));
    fs::write(&path, record.to_string()).unwrap();
    path
}
#[test]
fn claude_active_session_without_transcript_is_included_and_wrong_workspace_is_excluded() {
    let (engine, dir) = fixture(Provider::Claude);
    let native = uuid::Uuid::new_v4().to_string();
    current_registration(dir.path(), dir.path(), &native);
    current_registration(
        dir.path(),
        &dir.path().join("other"),
        &uuid::Uuid::new_v4().to_string(),
    );
    let result = claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    assert_eq!(result["imported"], 1);
    let snapshot = engine.store.snapshot().unwrap();
    assert_eq!(snapshot.runs.len(), 1);
    let run = &snapshot.runs[0];
    assert_eq!(run.state, RunState::Running);
    assert_eq!(run.session_key.as_deref(), Some(native.as_str()));
    assert_eq!(run.observation_source, "claude/local-session");
    assert!(!run.capabilities.send_to_active.supported);
}
#[test]
fn claude_active_history_is_restored_from_old_cleanup_but_dead_registration_stays_hidden() {
    let (engine, dir) = fixture(Provider::Claude);
    let native = uuid::Uuid::new_v4().to_string();
    let file = dir
        .path()
        .join("projects/fixture")
        .join(format!("{native}.jsonl"));
    write(
        &file,
        &[row(
            &native,
            dir.path(),
            "u",
            None,
            "user",
            json!("Keep my question"),
        )],
    );
    claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    let run = engine.store.snapshot().unwrap().runs.remove(0);
    engine.store.set_session_hidden(&run.id, true).unwrap();
    let path = current_registration(dir.path(), dir.path(), &native);
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record.as_object_mut().unwrap().remove("procStart");
    record.as_object_mut().unwrap().remove("procStartFt");
    fs::write(&path, record.to_string()).unwrap();
    claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    assert_eq!(engine.store.snapshot().unwrap().removed_sessions.len(), 1);
    assert_eq!(
        engine.store.run(&run.id).unwrap().state,
        RunState::Uncertain
    );
    record["pid"] = json!(0);
    fs::write(&path, record.to_string()).unwrap();
    claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    assert_eq!(engine.store.snapshot().unwrap().removed_sessions.len(), 1);
    current_registration(dir.path(), dir.path(), &native);
    claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    assert!(engine.store.snapshot().unwrap().removed_sessions.is_empty());
    assert_eq!(engine.store.run(&run.id).unwrap().state, RunState::Running);
    assert_eq!(
        engine.store.detail(&run.id).unwrap().messages[0].text,
        "Keep my question"
    );
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(file)
        .unwrap()
        .write_all(b"{partial")
        .unwrap();
    let result = claude_history::discover_in(&engine, "p", dir.path()).unwrap();
    assert_eq!(result["imported"], 1);
    assert_eq!(result["errors"], json!([]));
    assert_eq!(engine.store.run(&run.id).unwrap().state, RunState::Running);
}
