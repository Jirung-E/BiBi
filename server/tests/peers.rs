use bibi_core::*;
use bibi_server::{config::ServiceConfig, peer, router, runtime::Engine, state};
use std::time::Duration;

async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(12), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn peer_reconnect_keeps_one_run_and_mirrors_actual_input_delivery() {
    let central_dir = tempfile::tempdir().unwrap();
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = state(
        Store::memory().unwrap(),
        ServiceConfig::new(remote_dir.path().into()),
        "test-peer-token-at-least-32-characters".into(),
    );
    remote
        .store
        .save_provider(
            serde_json::from_value(
                serde_json::json!({"id":"remote-mock","name":"원격 모의","adapter":"mock"}),
            )
            .unwrap(),
            Some("remote-secret-fixture".into()),
        )
        .unwrap();
    remote.engine.start().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(axum::serve(listener, router(remote.clone())).into_future());
    let store = Store::open(central_dir.path().join("state.sqlite")).unwrap();
    let config = ServiceConfig::new(central_dir.path().into());
    let central = Engine::new(store.clone(), config.clone());
    central.start().await.unwrap();
    store
        .add_project(Project {
            id: "p".into(),
            name: "p".into(),
            workspace: central_dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    let host = peer::register(
        &central,
        "test host".into(),
        url,
        remote.token.as_ref().clone(),
        "p".into(),
        remote_dir.path().to_string_lossy().into(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(store.snapshot().unwrap().providers[0].host_id, host.id);
    assert!(
        !serde_json::to_string(&store.snapshot().unwrap())
            .unwrap()
            .contains("remote-secret-fixture")
    );
    let receipt = store
        .submit(Submission {
            submission_id: "job".into(),
            project_key: "p".into(),
            work_id: None,
            title: None,
            question: "오래 실행되는 모의 질문. ".repeat(90),
            provider: Provider::Mock,
            provider_id: Some(remote_provider_id(&host.id, "remote-mock")),
            model: "mock".into(),
            host_id: host.id.clone(),
            role: "DB".into(),
            mode: SubmitMode::Fresh,
            target_run_id: None,
            expected_turn_id: None,
            expected_context_revision: None,
            read_only: false,
        })
        .unwrap();
    until(|| store.run(&receipt.run_id).unwrap().turn_id.is_some()).await;
    let first = remote.store.run(&receipt.run_id).unwrap();
    assert_eq!(first.provider_id.as_deref(), Some("remote-mock"));
    let sent_at = store.snapshot().unwrap().transmissions[0].sent_at;
    central.stop().await;
    assert_eq!(
        remote.store.run(&receipt.run_id).unwrap().state,
        RunState::Running,
        "central shutdown must not terminate remote work"
    );
    store
        .update_context(
            &receipt.work_id,
            ContextUpdate {
                expected_revision: 1,
                goal: "updated after first delivery".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let restarted = Engine::new(store.clone(), config);
    restarted.start().await.unwrap();
    until(|| store.run(&receipt.run_id).unwrap().phase == "진행 중").await;
    store
        .submit(Submission {
            submission_id: "steer".into(),
            project_key: "p".into(),
            work_id: Some(receipt.work_id.clone()),
            title: None,
            question: "추가 지시".into(),
            provider: Provider::Mock,
            provider_id: Some(remote_provider_id(&host.id, "remote-mock")),
            model: "mock".into(),
            host_id: host.id.clone(),
            role: "DB".into(),
            mode: SubmitMode::Steer,
            target_run_id: Some(receipt.run_id.clone()),
            expected_turn_id: first.turn_id.clone(),
            expected_context_revision: Some(2),
            read_only: false,
        })
        .unwrap();
    until(|| {
        store
            .inputs(&receipt.run_id)
            .unwrap()
            .iter()
            .any(|i| i.state == "delivered")
    })
    .await;
    let before = remote.store.snapshot().unwrap();
    let direct = before
        .transmissions
        .iter()
        .find(|e| e.id == "steer:steer")
        .unwrap();
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .transmissions
            .iter()
            .find(|e| e.id == direct.id)
            .unwrap()
            .sent_at,
        direct.sent_at
    );
    restarted.interrupt(&receipt.run_id).await.unwrap();
    until(|| store.run(&receipt.run_id).unwrap().state == RunState::Interrupted).await;
    assert_eq!(remote.store.snapshot().unwrap().runs.len(), 1);
    assert_eq!(
        remote.store.run(&receipt.run_id).unwrap().session_key,
        first.session_key
    );
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .transmissions
            .iter()
            .find(|e| e.kind == "request")
            .unwrap()
            .sent_at,
        sent_at
    );
    let detail = store.detail(&receipt.run_id).unwrap();
    assert_eq!(
        detail.messages.iter().filter(|m| m.role == "user").count(),
        2
    );
    assert_eq!(
        detail.run.workspace,
        remote_dir.path().canonicalize().unwrap().to_string_lossy()
    );
    assert_eq!(store.work(&receipt.work_id).unwrap().context_revision, 2);
    assert!(
        !serde_json::to_string(&store.snapshot().unwrap())
            .unwrap()
            .contains("test-peer-token")
    );
    let child = remote
        .store
        .submit(Submission {
            submission_id: "remote-expert".into(),
            project_key: "p".into(),
            work_id: Some(receipt.work_id.clone()),
            title: None,
            question: "remote expert".into(),
            provider: Provider::Mock,
            provider_id: Some("remote-mock".into()),
            model: "mock".into(),
            host_id: "local".into(),
            role: "전문가: DB".into(),
            mode: SubmitMode::Fresh,
            target_run_id: Some(receipt.run_id.clone()),
            expected_turn_id: None,
            expected_context_revision: Some(1),
            read_only: true,
        })
        .unwrap();
    until(|| remote.store.run(&child.run_id).unwrap().state == RunState::Completed).await;
    peer::refresh(&restarted, host).await.unwrap();
    let mirrored = store.detail(&child.run_id).unwrap();
    assert_eq!(mirrored.run.state, RunState::Completed);
    assert_eq!(mirrored.run.parent_run_id, Some(receipt.run_id));
    assert_eq!(
        mirrored
            .inbox
            .iter()
            .filter(|entry| entry.from_run_id == child.run_id)
            .count(),
        1
    );
    assert_eq!(store.work(&child.work_id).unwrap().context_revision, 2);
    restarted.stop().await;
    remote.engine.stop().await;
    server.abort();
}

#[tokio::test]
async fn independently_created_remote_project_and_expert_continue_through_session_tools() {
    use bibi_server::providers::session_tools as task_tools;
    use serde_json::json;
    let local_dir = tempfile::tempdir().unwrap();
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = state(
        Store::memory().unwrap(),
        ServiceConfig::new(remote_dir.path().into()),
        "peer-auth-at-least-thirty-two-characters".into(),
    );
    remote.engine.start().await.unwrap();
    remote
        .store
        .add_project(Project {
            id: "remote-project".into(),
            name: "원래 프로젝트".into(),
            workspace: remote_dir
                .path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    fn submission(key: &str, project: &str) -> Submission {
        Submission {
            submission_id: key.into(),
            project_key: project.into(),
            work_id: None,
            title: None,
            question: "모의 질문".into(),
            provider: Provider::Mock,
            provider_id: None,
            model: "mock".into(),
            host_id: "local".into(),
            role: "조정".into(),
            mode: SubmitMode::Fresh,
            target_run_id: None,
            expected_turn_id: None,
            expected_context_revision: None,
            read_only: true,
        }
    }
    let first = remote
        .store
        .submit(submission("remote-first", "remote-project"))
        .unwrap();
    until(|| remote.store.run(&first.run_id).unwrap().state == RunState::Completed).await;
    let mut child_request = submission("child-first", "remote-project");
    child_request.target_run_id = Some(first.run_id.clone());
    child_request.expected_context_revision = Some(1);
    child_request.role = "전문가: 검토".into();
    let child = remote.store.submit(child_request.clone()).unwrap();
    until(|| remote.store.run(&child.run_id).unwrap().state == RunState::Completed).await;
    let child_native = remote.store.run(&child.run_id).unwrap();
    child_request.submission_id = "child-second".into();
    child_request.mode = SubmitMode::Continue;
    child_request.target_run_id = Some(child.run_id.clone());
    child_request.expected_turn_id = child_native.turn_id.clone();
    let continued = remote.store.submit(child_request).unwrap();
    until(|| remote.store.run(&continued.run_id).unwrap().state == RunState::Completed).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(axum::serve(listener, router(remote.clone())).into_future());
    let central = Engine::new(
        Store::memory().unwrap(),
        ServiceConfig::new(local_dir.path().into()),
    );
    central.start().await.unwrap();
    central
        .store
        .add_project(Project {
            id: "local-project".into(),
            name: "이쪽 프로젝트".into(),
            workspace: local_dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    let host = peer::register(
        &central,
        "다른 PC".into(),
        url,
        remote.token.as_ref().clone(),
        "local-project".into(),
        remote_dir.path().to_string_lossy().into(),
        None,
    )
    .await
    .unwrap();
    let imported = central.store.detail(&continued.run_id).unwrap();
    assert_eq!(imported.run.project_key, "local-project");
    assert_eq!(imported.run.session_id(), child_native.session_id());
    assert!(imported.run.capabilities.continue_session.supported);
    assert_eq!(
        imported
            .conversation
            .iter()
            .filter(|m| m.role == "user")
            .count(),
        2
    );
    assert_eq!(
        central.store.detail(&first.run_id).unwrap().children.len(),
        1
    );
    let source = central
        .store
        .submit(submission("local-source", "local-project"))
        .unwrap();
    until(|| central.store.run(&source.run_id).unwrap().state == RunState::Completed).await;
    let source = central.store.run(&source.run_id).unwrap();
    let mut sends = 0;
    let sessions = task_tools::execute(
        &central,
        &source,
        "sessions",
        &json!({"host_id":host.id}),
        &mut sends,
    )
    .await
    .unwrap();
    assert!(
        sessions["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["run_id"] == continued.run_id)
    );
    let args = json!({"run_id":continued.run_id,"submission_id":"ask-once","message":"기존 대화에 답해주세요"});
    let sent = task_tools::execute(&central, &source, "send_session", &args, &mut sends)
        .await
        .unwrap();
    let receipt: Receipt = serde_json::from_value(sent.clone()).unwrap();
    until(|| central.store.run(&receipt.run_id).unwrap().state == RunState::Completed).await;
    let repeat = task_tools::execute(&central, &source, "send_session", &args, &mut sends)
        .await
        .unwrap();
    assert_eq!(sent, repeat);
    let latest = remote.store.run(&receipt.run_id).unwrap();
    assert_eq!(latest.project_key, "remote-project");
    assert_eq!(latest.session_id(), child_native.session_id());
    assert_eq!(latest.session_key, child_native.session_key);
    assert_eq!(
        remote.store.project("remote-project").unwrap().name,
        "원래 프로젝트"
    );
    assert_eq!(remote.store.projects().unwrap().len(), 1);
    let answer = task_tools::execute(
        &central,
        &source,
        "session_result",
        &json!({"submission_id":receipt.submission_id,"wait":true}),
        &mut sends,
    )
    .await
    .unwrap();
    assert_eq!(answer["state"], "completed");
    assert_eq!(answer["results"].as_array().unwrap().len(), 1);
    peer::refresh(&central, host).await.unwrap();
    let messages = central.store.detail(&receipt.run_id).unwrap().conversation;
    assert_eq!(messages.iter().filter(|m| m.role == "user").count(), 3);
    assert!(
        messages
            .iter()
            .any(|m| m.text.contains("BiBi peer message") && m.text.contains(source.session_id()))
    );
    let mut changed = args.clone();
    changed["message"] = json!("다른 내용");
    assert!(
        task_tools::execute(&central, &source, "send_session", &changed, &mut sends)
            .await
            .is_err()
    );
    assert!(
        task_tools::execute(&central, &child_native, "send_session", &args, &mut sends)
            .await
            .is_err()
    );
    central.stop().await;
    remote.engine.stop().await;
    server.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn existing_remote_permission_is_delivered_to_its_original_runtime() {
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let local_dir = tempfile::tempdir().unwrap();
    let remote = state(
        Store::memory().unwrap(),
        ServiceConfig::new(dir.path().into()),
        "approval-test-token-at-least-32-characters".into(),
    );
    let workspace = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    remote
        .store
        .add_project(Project {
            id: "original".into(),
            name: "원격".into(),
            workspace: workspace.clone(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    remote.store.save_provider(serde_json::from_value(json!({"id":"fixture","name":"모의 프로토콜","adapter":"claude","command":format!("{}/tests/fixtures/claude.py",env!("CARGO_MANIFEST_DIR"))})).unwrap(),None).unwrap();
    remote.engine.start().await.unwrap();
    let receipt = remote
        .store
        .submit(Submission {
            submission_id: "permission-fixture".into(),
            project_key: "original".into(),
            work_id: None,
            title: None,
            question: "FIRST_FIXTURE".into(),
            provider: Provider::Claude,
            provider_id: Some("fixture".into()),
            model: String::new(),
            host_id: "local".into(),
            role: "업무 조정".into(),
            mode: SubmitMode::Fresh,
            target_run_id: None,
            expected_turn_id: None,
            expected_context_revision: None,
            read_only: false,
        })
        .unwrap();
    until(|| {
        !remote
            .store
            .detail(&receipt.run_id)
            .unwrap()
            .approvals
            .is_empty()
    })
    .await;
    let native = remote.store.run(&receipt.run_id).unwrap().session_key;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(axum::serve(listener, router(remote.clone())).into_future());
    let central = Engine::new(
        Store::memory().unwrap(),
        ServiceConfig::new(local_dir.path().into()),
    );
    central
        .store
        .add_project(Project {
            id: "mapped".into(),
            name: "로컬".into(),
            workspace: local_dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    central.start().await.unwrap();
    let host = peer::register(
        &central,
        "승인 호스트".into(),
        url,
        remote.token.as_ref().clone(),
        "mapped".into(),
        workspace,
        None,
    )
    .await
    .unwrap();
    until(|| {
        central
            .store
            .detail(&receipt.run_id)
            .is_ok_and(|d| !d.approvals.is_empty())
    })
    .await;
    let approval = central.store.detail(&receipt.run_id).unwrap().approvals[0].clone();
    assert_eq!(approval.state, "pending");
    // Observer attachment is asynchronous; retry only before a response is accepted.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match central
                .respond(&approval.id, json!({"decision":"accept"}))
                .await
            {
                Ok(()) => break,
                Err(error) if error.to_string().contains("연결되어 있지") => {
                    tokio::time::sleep(Duration::from_millis(20)).await
                }
                Err(error) => panic!("unexpected approval failure: {error}"),
            }
        }
    })
    .await
    .unwrap();
    until(|| central.store.run(&receipt.run_id).unwrap().state == RunState::Completed).await;
    peer::refresh(&central, host).await.unwrap();
    let detail = central.store.detail(&receipt.run_id).unwrap();
    assert_eq!(detail.approvals[0].state, "delivered");
    assert_eq!(detail.run.session_key, native);
    assert_eq!(
        remote.store.detail(&receipt.run_id).unwrap().approvals[0].state,
        "delivered"
    );
    assert!(
        central
            .respond(&approval.id, json!({"decision":"accept"}))
            .await
            .is_err()
    );
    assert_eq!(detail.children.len(), 1);
    central.stop().await;
    remote.engine.stop().await;
    server.abort();
}
