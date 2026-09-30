use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bibi_core::*;
use bibi_server::{config::ServiceConfig, router, state};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
fn app() -> (axum::Router, Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().unwrap();
    let config = ServiceConfig::new(dir.path().into());
    let app = router(state(
        store.clone(),
        config,
        "test-token-with-at-least-32-characters".into(),
    ));
    (app, store, dir)
}
#[tokio::test]
async fn private_reads_require_authentication_and_wrong_origin_is_rejected() {
    let (app, _, _dir) = app();
    let result = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/snapshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::UNAUTHORIZED);
    let result = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/snapshot")
                .header(
                    "authorization",
                    "Bearer test-token-with-at-least-32-characters",
                )
                .header("host", "localhost:44880")
                .header("origin", "https://attacker.invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::FORBIDDEN);
    let result = app
        .oneshot(
            Request::builder()
                .uri("/api/snapshot")
                .header(
                    "authorization",
                    "Bearer test-token-with-at-least-32-characters",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
}
#[tokio::test]
async fn browser_cookie_login_logout_and_no_secret_in_snapshot() {
    let (app, _, _dir) = app();
    let result = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"token":"test-token-with-at-least-32-characters"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let cookie = result.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let result = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/snapshot")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let body = String::from_utf8(
        result
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(!body.contains("test-token"));
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let result = app
        .oneshot(
            Request::builder()
                .uri("/api/snapshot")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn http_submission_retry_and_event_replay_match_core_contract() {
    let (app, store, dir) = app();
    store
        .add_project(Project {
            id: "p".into(),
            name: "p".into(),
            workspace: dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    store
        .upsert_host(Host {
            id: "local".into(),
            name: "test".into(),
            platform: "test".into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![Provider::Mock],
            error: None,
        })
        .unwrap();
    let body = json!({"type":"submit","request":{"submission_id":"same","project_key":"p","work_id":null,"title":null,"question":"hello",
        "provider":"mock","model":"mock","host_id":"local","role":"업무 조정","mode":"fresh","target_run_id":null,
        "expected_turn_id":null,"expected_context_revision":null}});
    let mut run_id = String::new();
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/command")
                    .header("content-type", "application/json")
                    .header(
                        "authorization",
                        "Bearer test-token-with-at-least-32-characters",
                    )
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        if run_id.is_empty() {
            run_id = value["run_id"].as_str().unwrap().into();
        } else {
            assert_eq!(value["run_id"], run_id);
        }
    }
    assert_eq!(store.snapshot().unwrap().runs.len(), 1);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/events?after=0")
                .header(
                    "authorization",
                    "Bearer test-token-with-at-least-32-characters",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let value: Vec<Event> =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(value.windows(2).all(|pair| pair[0].seq < pair[1].seq));
}
#[tokio::test]
async fn mock_runtime_persists_output_and_result_without_model_process() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().unwrap();
    store
        .add_project(Project {
            id: "p".into(),
            name: "p".into(),
            workspace: dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    let app = state(
        store.clone(),
        ServiceConfig::new(dir.path().into()),
        "token".into(),
    );
    app.engine.start().await.unwrap();
    let receipt = store
        .submit(Submission {
            submission_id: "s".into(),
            project_key: "p".into(),
            work_id: None,
            title: None,
            question: "질문".into(),
            provider: Provider::Mock,
            provider_id: None,
            model: "mock".into(),
            host_id: "local".into(),
            role: "DB".into(),
            mode: SubmitMode::Fresh,
            target_run_id: None,
            expected_turn_id: None,
            expected_context_revision: None,
            read_only: false,
            approval_mode: None,
        })
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if store.run(&receipt.run_id).unwrap().state == RunState::Completed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let detail = store.detail(&receipt.run_id).unwrap();
    assert!(
        !detail
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text
            .is_empty()
    );
    assert_eq!(detail.inbox.len(), 1);
    assert!(detail.inbox[0].result.starts_with("모의 실행"));
}

#[tokio::test]
async fn disconnected_cleanup_uses_authenticated_command_without_starting_or_stopping_runs() {
    let (app, store, _dir) = app();
    store
        .add_project(Project {
            id: "cleanup".into(),
            name: "cleanup".into(),
            workspace: "/fixture".into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    store
        .upsert_host(Host {
            id: "local".into(),
            name: "fixture".into(),
            platform: "fixture".into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![Provider::Mock],
            error: None,
        })
        .unwrap();
    let request: Submission=serde_json::from_value(json!({"submission_id":"cleanup-run","project_key":"cleanup","question":"keep","provider":"mock","model":"fixture","host_id":"local","role":"coordinator","mode":"fresh"})).unwrap();
    let receipt = store.submit(request).unwrap();
    store
        .observe(
            &receipt.run_id,
            RunState::Disconnected,
            "disconnected",
            None,
        )
        .unwrap();
    let body = json!({"type":"cleanup_disconnected_sessions","project_key":"cleanup","run_ids":[receipt.run_id]});
    let req = |auth: bool| {
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/command")
            .header("content-type", "application/json");
        if auth {
            req = req.header(
                "authorization",
                "Bearer test-token-with-at-least-32-characters",
            );
        }
        req.body(Body::from(body.to_string())).unwrap()
    };
    assert_eq!(
        app.clone().oneshot(req(false)).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(store.snapshot().unwrap().removed_sessions.is_empty());
    let response = app.oneshot(req(true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        result["hidden_session_ids"],
        json!([store.run(&receipt.run_id).unwrap().session_id()])
    );
    assert_eq!(result["skipped_run_ids"], json!([]));
    let snap = store.snapshot().unwrap();
    assert!(snap.session_cleanup_v1);
    assert!(snap.runs.is_empty());
    assert_eq!(snap.removed_sessions[0].state, RunState::Disconnected);
    assert_eq!(
        store.detail(&receipt.run_id).unwrap().messages[0].text,
        "keep"
    );
}
