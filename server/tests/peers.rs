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
    let receipt = store
        .submit(Submission {
            submission_id: "job".into(),
            project_key: "p".into(),
            work_id: None,
            title: None,
            question: "오래 실행되는 모의 질문. ".repeat(90),
            provider: Provider::Mock,
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
