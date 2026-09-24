use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use bibi_core::{ProviderConfig, Store};
use bibi_server::{config::ServiceConfig, providers::check::check, router, runtime::Engine, state};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn provider(adapter: &str, endpoint: &str) -> ProviderConfig {
    serde_json::from_value(json!({"id":"draft","name":"","adapter":adapter,"endpoint":endpoint}))
        .unwrap()
}
fn engine(dir: &tempfile::TempDir) -> Engine {
    Engine::new(
        Store::memory().unwrap(),
        ServiceConfig::new(dir.path().into()),
    )
}
async fn fixture(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", socket.local_addr().unwrap());
    (
        base,
        tokio::spawn(async move {
            axum::serve(socket, app).await.unwrap();
        }),
    )
}
fn cli(dir: &tempfile::TempDir, adapter: &str, mode: &str) -> ProviderConfig {
    let mut p = provider(adapter, "");
    p.command = "node".into();
    p.args = vec![
        format!(
            "{}/tests/fixtures/provider-check.mjs",
            env!("CARGO_MANIFEST_DIR")
        ),
        mode.into(),
        dir.path().join(mode).to_string_lossy().into_owned(),
    ];
    p
}

#[tokio::test]
async fn unsaved_ollama_probe_reads_models_without_mutating_any_store_state() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&dir);
    let before = serde_json::to_value(e.store.snapshot().unwrap()).unwrap();
    let (base, task) = fixture(Router::new().route(
        "/api/tags",
        get(|| async {
            Json(json!({"models":[{"name":"gemma4:e4b"},{"name":"gemma4:e4b"},{"name":"another"}]}))
        }),
    ))
    .await;
    let result = check(&e, &provider("ollama", &(base + "/")), None).await;
    assert!(result.ok, "{}", result.message);
    assert_eq!(result.models, ["another", "gemma4:e4b"]);
    assert!(result.message.contains("2개"));
    assert_eq!(
        serde_json::to_value(e.store.snapshot().unwrap()).unwrap(),
        before
    );
    task.abort();
}

#[tokio::test]
async fn api_checks_auth_schema_errors_redirects_and_empty_lists_honestly() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&dir);
    for (status, data, ok, hint) in [
        (200, json!({"models":[]}), true, "모델 없음"),
        (200, json!({"not_models":[]}), false, "형식"),
        (200, json!({"models":[{}]}), false, "이름"),
        (401, json!({"error":"secret-do-not-echo"}), false, "인증"),
        (404, json!({}), false, "404"),
        (302, json!({}), false, "이동"),
    ] {
        let (base, task) = fixture(Router::new().route(
            "/api/tags",
            get(move || async move {
                (
                    StatusCode::from_u16(status).unwrap(),
                    [("location", "http://never-follow.invalid/")],
                    Json(data),
                )
            }),
        ))
        .await;
        let result = check(&e, &provider("ollama", &base), None).await;
        assert_eq!(result.ok, ok);
        assert!(result.message.contains(hint), "{}", result.message);
        assert!(!result.message.contains("secret-do-not-echo"));
        task.abort();
    }
    let (base, task) =
        fixture(Router::new().route("/api/tags", get(|| async { "<html>not an API</html>" })))
            .await;
    let result = check(&e, &provider("ollama", &base), None).await;
    assert!(!result.ok);
    assert!(result.message.contains("JSON"));
    task.abort();
}

#[tokio::test]
async fn api_response_size_and_slow_connection_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&dir);
    let (base, task) = fixture(
        Router::new()
            .route(
                "/large/api/tags",
                get(|| async { "x".repeat(1024 * 1024 + 1) }),
            )
            .route(
                "/slow/api/tags",
                get(|| async {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    Json(json!({"models":[]}))
                }),
            ),
    )
    .await;
    let result = check(&e, &provider("ollama", &format!("{base}/large")), None).await;
    assert!(!result.ok);
    assert!(result.message.contains("너무 큽니다"));
    let result = check(&e, &provider("ollama", &format!("{base}/slow")), None).await;
    assert!(!result.ok);
    assert!(result.message.contains("시간"));
    task.abort();
}

#[tokio::test]
async fn stored_key_is_reused_only_for_its_saved_destination_and_never_changed() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&dir);
    let seen = Arc::new(Mutex::new(vec![]));
    let requests = seen.clone();
    let (base, task) = fixture(Router::new().route(
        "/{*path}",
        get(move |request: Request<Body>| {
            let requests = requests.clone();
            async move {
                requests.lock().unwrap().push((
                    request.uri().path().to_owned(),
                    request
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned),
                ));
                Json(json!({"data":[{"id":"fixture-model"}]}))
            }
        }),
    ))
    .await;
    let mut p = provider("open_ai", &format!("{base}/v1/chat/completions"));
    p.name = "Fixture".into();
    e.store
        .save_provider(p.clone(), Some("stored-secret".into()))
        .unwrap();
    let before = serde_json::to_value(e.store.snapshot().unwrap()).unwrap();
    assert!(check(&e, &p, None).await.ok);
    assert_eq!(
        seen.lock().unwrap()[0],
        ("/v1/models".into(), Some("Bearer stored-secret".into()))
    );
    p.endpoint = format!("{base}/different");
    let denied = check(&e, &p, None).await;
    assert!(!denied.ok);
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(check(&e, &p, Some("new-secret".into())).await.ok);
    assert!(check(&e, &p, Some("".into())).await.ok);
    assert_eq!(seen.lock().unwrap()[2].1, None);
    assert_eq!(
        e.store.provider_secret("draft").unwrap().as_deref(),
        Some("stored-secret")
    );
    assert_eq!(
        serde_json::to_value(e.store.snapshot().unwrap()).unwrap(),
        before
    );
    task.abort();
}

#[tokio::test]
async fn cli_checks_only_control_or_auth_and_do_not_create_turns() {
    for (adapter, mode, ok) in [
        ("codex", "codex", true),
        ("codex", "codex-logged-out", false),
        ("claude", "claude", true),
        ("claude", "claude-logged-out", false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let e = engine(&dir);
        let result = check(&e, &cli(&dir, adapter, mode), None).await;
        assert_eq!(result.ok, ok, "{}", result.message);
        assert!(!result.message.contains("@"));
        let record = std::fs::read_to_string(dir.path().join(mode)).unwrap();
        let lines: Vec<Value> = record
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        if adapter == "codex" {
            assert_eq!(
                &lines[1..],
                &[
                    json!("initialize"),
                    json!("initialized"),
                    json!("account/read")
                ]
            );
        } else {
            assert_eq!(lines, vec![json!(["auth", "status", "--json"])]);
        }
        let snapshot = e.store.snapshot().unwrap();
        assert!(snapshot.runs.is_empty());
        assert!(snapshot.providers.is_empty());
        assert!(snapshot.quotas.is_empty());
    }
}

#[tokio::test]
async fn cli_timeout_missing_command_and_unsupported_adapter_do_not_report_success() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&dir);
    let result = check(&e, &cli(&dir, "claude", "hang"), None).await;
    assert!(!result.ok);
    assert!(result.message.contains("초과"));
    let mut p = provider("claude", "");
    p.command = "bibi-nonexistent-provider-command-fixture".into();
    assert!(!check(&e, &p, None).await.ok);
    let p = cli(&dir, "command", "unsupported");
    assert!(!check(&e, &p, None).await.ok);
    assert!(!dir.path().join("unsupported").exists());
}

#[tokio::test]
async fn unsafe_urls_and_remote_drafts_are_rejected_without_reflecting_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&dir);
    for endpoint in [
        "file:///tmp/test",
        "http://u:secret@127.0.0.1",
        "http://127.0.0.1/?key=secret",
        "http://127.0.0.1/#secret",
    ] {
        let result = check(&e, &provider("ollama", endpoint), None).await;
        assert!(!result.ok);
        assert!(!result.message.contains("secret"));
    }
    let mut p = provider("ollama", "http://127.0.0.1:1");
    p.host_id = "remote".into();
    let result = check(&e, &p, None).await;
    assert!(!result.ok);
    assert!(result.message.contains("호스트"));
}

#[tokio::test]
async fn probe_command_requires_auth_and_returns_report_without_saving_provider() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().unwrap();
    let app = router(state(
        store.clone(),
        ServiceConfig::new(dir.path().into()),
        "fixture-token".into(),
    ));
    let body = json!({"type":"check_provider","provider":provider("mock","")}).to_string();
    async fn send(app: Router, body: String, authorized: bool) -> Response {
        let mut builder = Request::builder()
            .uri("/api/command")
            .method("POST")
            .header("content-type", "application/json");
        if authorized {
            builder = builder.header("authorization", "Bearer fixture-token");
        }
        app.oneshot(builder.body(Body::from(body)).unwrap())
            .await
            .unwrap()
            .into_response()
    }
    assert_eq!(
        send(app.clone(), body.clone(), false).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = send(app, body, true).await;
    assert_eq!(response.status(), StatusCode::OK);
    let data: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(data["ok"], false);
    assert!(store.providers().unwrap().is_empty());
}
