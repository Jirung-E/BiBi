use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    routing::post,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bibi_core::*;
use bibi_server::{
    attachments::{self, Upload},
    config::ServiceConfig,
    runtime::Engine,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tower::ServiceExt;
const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6h0AAAAASUVORK5CYII=";

fn fixture() -> (Engine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.sqlite3")).unwrap();
    store
        .add_project(Project {
            id: "p".into(),
            name: "fixture".into(),
            workspace: dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    // No runtime is started here; the local HTTP and native fixtures are explicit.
    store
        .upsert_host(Host {
            id: "local".into(),
            name: "local".into(),
            platform: "fixture".into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![
                Provider::Mock,
                Provider::Claude,
                Provider::Codex,
                Provider::Ollama,
                Provider::OpenAi,
                Provider::Command,
            ],
            error: None,
        })
        .unwrap();
    (
        Engine::new(store, ServiceConfig::new(dir.path().into())),
        dir,
    )
}
fn file(e: &Engine, name: &str, data: &[u8]) -> Attachment {
    attachments::store_upload(
        &e.store,
        Upload {
            id: id("att"),
            project_key: "p".into(),
            name: name.into(),
            data_base64: STANDARD.encode(data),
        },
    )
    .unwrap()
}
fn request(provider: Provider, files: &[Attachment]) -> Submission {
    serde_json::from_value(json!({"submission_id":id("send"),"project_key":"p","question":"","provider":provider,"model":"fixture","host_id":"local","role":"coordinator","mode":"fresh","read_only":false,"attachments":files.iter().map(|a|&a.id).collect::<Vec<_>>()})).unwrap()
}
fn next(e: &Engine, previous: &Run, files: &[Attachment]) -> Submission {
    let mut r = request(previous.provider.clone(), files);
    r.question = "next question".into();
    r.provider_id = previous.provider_id.clone();
    r.model = previous.model.clone();
    r.mode = SubmitMode::Continue;
    r.target_run_id = Some(previous.id.clone());
    r.expected_turn_id = previous.turn_id.clone();
    r.expected_context_revision = Some(e.store.work(&previous.work_id).unwrap().context_revision);
    r.read_only = previous.read_only;
    r
}
async fn finished(e: &Engine, r: &Receipt) -> RunDetail {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let d = e.store.detail(&r.run_id).unwrap();
            if d.run.state.terminal() {
                assert_eq!(d.run.state, RunState::Completed, "{:?}", d.run.error);
                return d;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("attachment fixture timeout")
}

#[test]
fn upload_is_immutable_project_scoped_bounded_and_survives_restart() {
    let (e, dir) = fixture();
    let a = file(&e, "이미지.png", &STANDARD.decode(PNG).unwrap());
    assert_eq!(a.media_type, "image/png");
    let again = attachments::store_upload(
        &e.store,
        Upload {
            id: a.id.clone(),
            project_key: "p".into(),
            name: a.name.clone(),
            data_base64: PNG.into(),
        },
    )
    .unwrap();
    assert_eq!(a, again);
    assert!(
        attachments::store_upload(
            &e.store,
            Upload {
                id: a.id.clone(),
                project_key: "p".into(),
                name: a.name.clone(),
                data_base64: STANDARD.encode(b"different")
            }
        )
        .is_err()
    );
    assert!(e.store.attachment_data(&a.id, "other-project").is_err());
    for name in ["../escape.png", "..\\escape.png", "bad\nname", ""] {
        assert!(
            attachments::store_upload(
                &e.store,
                Upload {
                    id: id("att"),
                    project_key: "p".into(),
                    name: name.into(),
                    data_base64: PNG.into()
                }
            )
            .is_err()
        );
    }
    assert!(
        attachments::store_upload(
            &e.store,
            Upload {
                id: "att_../../bad".into(),
                project_key: "p".into(),
                name: "safe".into(),
                data_base64: PNG.into()
            }
        )
        .is_err()
    );
    assert!(
        attachments::store_upload(
            &e.store,
            Upload {
                id: id("att"),
                project_key: "p".into(),
                name: "huge".into(),
                data_base64: STANDARD.encode(vec![1; attachments::MAX_FILE + 1])
            }
        )
        .is_err()
    );
    let svg = file(&e, "bad.svg", b"<svg onload='alert(1)'></svg>");
    assert_eq!(svg.kind, "text");
    assert_eq!(svg.media_type, "text/plain");
    drop(e);
    let store = Store::open(dir.path().join("test.sqlite3")).unwrap();
    assert_eq!(
        store.attachment_data(&a.id, "p").unwrap().1,
        STANDARD.decode(PNG).unwrap()
    );
}

#[test]
fn attachments_only_submission_is_atomic_idempotent_and_rejects_unsupported_inputs() {
    let (e, _) = fixture();
    let a = file(&e, "reference.txt", "hello 한글".as_bytes());
    let b = file(&e, "binary.zip", b"PK\x03\x04\0");
    let r = request(Provider::Mock, std::slice::from_ref(&a));
    let receipt = e.store.submit(r.clone()).unwrap();
    assert_eq!(receipt.run_id, e.store.submit(r).unwrap().run_id);
    let d = e.store.detail(&receipt.run_id).unwrap();
    assert_eq!(d.messages[0].attachments, vec![a.clone()]);
    assert_eq!(d.run.title, "reference.txt");
    assert_eq!(d.run.context.attachments, vec![a.clone()]);
    assert!(
        !serde_json::to_string(&e.store.snapshot().unwrap())
            .unwrap()
            .contains("hello 한글")
    );
    for mut r in [
        request(Provider::Ollama, std::slice::from_ref(&b)),
        request(Provider::OpenAi, std::slice::from_ref(&b)),
        request(Provider::Command, std::slice::from_ref(&a)),
        request(Provider::Mock, &[a.clone(), a.clone()]),
    ] {
        r.question = "x".into();
        assert!(e.store.submit(r).is_err());
    }
    let mut slash = request(Provider::Claude, std::slice::from_ref(&a));
    slash.question = "/compact".into();
    assert!(e.store.submit(slash).is_err());
    let mut missing = request(Provider::Mock, &[a]);
    missing.attachments = vec![id("att")];
    assert!(e.store.submit(missing).is_err());
}

#[tokio::test]
async fn upload_download_use_authenticated_http_and_local_json_transport() {
    let (e, _) = fixture();
    let app = bibi_server::router(bibi_server::state(
        e.store.clone(),
        e.config.clone(),
        "secret-fixture-token".into(),
    ));
    let body = json!({"id":id("att"),"project_key":"p","name":"photo.png","data_base64":PNG});
    let make = |authorized: bool| {
        let mut r = Request::builder()
            .uri("/api/attachments")
            .method("POST")
            .header("content-type", "application/json");
        if authorized {
            r = r.header("authorization", "Bearer secret-fixture-token");
        }
        r.body(Body::from(body.to_string())).unwrap()
    };
    assert_eq!(
        app.clone().oneshot(make(false)).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let response = app.clone().oneshot(make(true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let meta: Attachment =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let get = |project: &str| {
        Request::builder()
            .uri(format!(
                "/api/attachments/{}?project_key={project}",
                meta.id
            ))
            .header("authorization", "Bearer secret-fixture-token")
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(get("other")).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let response = app.oneshot(get("p")).await.unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["data_base64"], PNG);
    // Desktop's existing in-memory router supports the same upload without TCP.
    let dir = tempfile::tempdir().unwrap();
    let local = bibi_server::local::LocalService::open(ServiceConfig::new(dir.path().into()))
        .await
        .unwrap();
    local
        .app
        .store
        .add_project(e.store.project("p").unwrap())
        .unwrap();
    let (status, value) = local
        .request("/api/attachments", "POST", body)
        .await
        .unwrap();
    assert_eq!(status, 200);
    let (status, value) = local
        .request(
            &format!(
                "/api/attachments/{}?project_key=p",
                value["id"].as_str().unwrap()
            ),
            "GET",
            Value::Null,
        )
        .await
        .unwrap();
    assert_eq!(status, 200);
    assert_eq!(value["data_base64"], PNG);
    local.shutdown().await;
}

#[tokio::test]
async fn native_providers_receive_real_attachment_payloads_and_continue_the_same_session() {
    for provider in [Provider::Claude, Provider::Codex] {
        let (mut e, _dir) = fixture();
        let script = format!(
            "{}/tests/fixtures/attachments.mjs",
            env!("CARGO_MANIFEST_DIR")
        );
        match provider {
            Provider::Claude => {
                e.config.claude_command = "node".into();
                e.config.claude_args = vec![script, "claude".into()];
            }
            _ => {
                e.config.codex_command = "node".into();
                e.config.codex_args = vec![script, "codex".into()];
            }
        }
        let img = file(&e, "pixel.png", &STANDARD.decode(PNG).unwrap());
        let txt = file(&e, "notes.md", b"ATTACHMENT_FILE_CONTENT");
        let pdf = file(&e, "notes.pdf", b"%PDF-1.4\nfixture\n%%EOF");
        let bin = file(&e, "archive.zip", b"PK\x03\x04\0fixture");
        e.start().await.unwrap();
        let first = e
            .store
            .submit(request(provider.clone(), &[img.clone(), txt, pdf, bin]))
            .unwrap();
        let first = finished(&e, &first).await;
        assert_eq!(first.messages[0].attachments.len(), 4);
        let native = first.run.session_key.clone();
        let second = e.store.submit(next(&e, &first.run, &[img])).unwrap();
        let second = finished(&e, &second).await;
        assert_eq!(second.run.session_key, native);
        assert_eq!(second.run.session_id, first.run.session_id);
        assert_eq!(
            second
                .conversation
                .iter()
                .filter(|m| !m.attachments.is_empty())
                .count(),
            2
        );
        e.stop().await;
    }
}

#[tokio::test]
async fn api_provider_requests_keep_attachments_in_followups_and_forks() {
    for adapter in [Provider::OpenAi, Provider::Ollama] {
        let (mut e, _dir) = fixture();
        let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
        async fn chat(
            State(seen): State<Arc<Mutex<Vec<Value>>>>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            seen.lock().unwrap().push(body);
            Json(
                json!({"choices":[{"message":{"content":"fixture answer"},"finish_reason":"stop"}],"message":{"role":"assistant","content":"fixture answer"},"done":true}),
            )
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new()
                    .route("/chat/completions", post(chat))
                    .route("/api/chat", post(chat))
                    .with_state(captured.clone()),
            )
            .into_future(),
        );
        let p: ProviderConfig = serde_json::from_value(
            json!({"id":"fixture-provider","name":"fixture","adapter":adapter,"endpoint":url}),
        )
        .unwrap();
        e.store.save_provider(p, None).unwrap();
        let image = file(&e, "picture.png", &STANDARD.decode(PNG).unwrap());
        let text = file(&e, "notes.txt", b"API_FILE_CONTENT");
        e.config.ollama_url = url;
        e.start().await.unwrap();
        let mut r = request(adapter.clone(), &[image.clone(), text]);
        r.provider_id = Some("fixture-provider".into());
        let first = finished(&e, &e.store.submit(r).unwrap()).await;
        let second = finished(&e, &e.store.submit(next(&e, &first.run, &[])).unwrap()).await;
        let requests = captured.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        let follow = serde_json::to_string(&requests[1]).unwrap();
        assert!(follow.contains(PNG));
        assert!(follow.contains("API_FILE_CONTENT"));
        assert!(!follow.contains("_bibi_images"));
        if adapter == Provider::Ollama {
            let stored = e
                .store
                .setting::<Value>(&format!("ollama:history:{}", second.run.id))
                .unwrap()
                .unwrap();
            assert!(!stored.to_string().contains(PNG));
            assert!(stored.to_string().contains(&image.id));
        }
        let points = bibi_server::providers::forks::points(&e, &second.run.id)
            .await
            .unwrap();
        let p = &points["points"][0];
        let fork = bibi_server::providers::forks::fork(
            &e,
            &ForkRequest {
                request_id: id("fork"),
                run_id: second.run.id.clone(),
                point_id: p["id"].as_str().unwrap().into(),
                revision: p["revision"].as_str().unwrap().into(),
                title: "fork".into(),
            },
        )
        .await
        .unwrap();
        assert!(
            e.store
                .detail(&fork.id)
                .unwrap()
                .conversation
                .iter()
                .any(|m| m.attachments.iter().any(|a| a.id == image.id))
        );
        finished(&e, &e.store.submit(next(&e, &fork, &[])).unwrap()).await;
        assert!(
            captured
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .to_string()
                .contains(PNG)
        );
        e.stop().await;
        server.abort();
    }
}

#[test]
fn materialized_attachments_are_concurrent_and_tamper_checked_without_empty_text_blocks() {
    let (engine, _dir) = fixture();
    let image = file(&engine, "picture.png", &STANDARD.decode(PNG).unwrap());
    let barrier = std::sync::Barrier::new(8);
    let paths = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    let input = attachments::codex_input(&engine, "", std::slice::from_ref(&image))
                        .unwrap();
                    assert!(!input.as_array().unwrap().iter().any(|i| i["text"] == ""));
                    input
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|i| i["type"] == "localImage")
                        .unwrap()["path"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(paths.iter().all(|p| p == &paths[0]));
    assert_eq!(
        std::fs::read(&paths[0]).unwrap(),
        STANDARD.decode(PNG).unwrap()
    );
    for content in [
        attachments::claude_content(&engine, "", std::slice::from_ref(&image)).unwrap(),
        attachments::openai_content(&engine, "", std::slice::from_ref(&image)).unwrap(),
    ] {
        assert!(!content.as_array().unwrap().iter().any(|i| i["text"] == ""));
        assert!(content.to_string().contains(PNG));
    }
    std::fs::write(&paths[0], b"changed").unwrap();
    assert!(attachments::codex_input(&engine, "x", &[image]).is_err());
    assert_eq!(std::fs::read(&paths[0]).unwrap(), b"changed");
}

#[tokio::test]
async fn codex_receives_attachment_when_steering_and_records_it_once() {
    let (mut e, _dir) = fixture();
    e.config.codex_command = "node".into();
    e.config.codex_args = vec![
        format!(
            "{}/tests/fixtures/attachments.mjs",
            env!("CARGO_MANIFEST_DIR")
        ),
        "codex".into(),
        "steer".into(),
    ];
    let image = file(&e, "steer.png", &STANDARD.decode(PNG).unwrap());
    e.start().await.unwrap();
    let mut first = request(Provider::Codex, &[]);
    first.question = "wait for steering".into();
    let receipt = e.store.submit(first).unwrap();
    let active = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let run = e.store.run(&receipt.run_id).unwrap();
            assert!(!run.state.terminal(), "{:?}", run.error);
            if run.turn_id.as_deref() == Some("turn-1") {
                break run;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let mut steering = next(&e, &active, std::slice::from_ref(&image));
    steering.mode = SubmitMode::Steer;
    steering.question.clear();
    let steering_id = steering.submission_id.clone();
    let accepted = e.store.submit(steering.clone()).unwrap();
    assert_eq!(accepted.run_id, active.id);
    assert_eq!(e.store.submit(steering).unwrap().run_id, active.id);
    let done = finished(&e, &receipt).await;
    assert_eq!(
        done.inputs
            .iter()
            .find(|i| i.id == steering_id)
            .unwrap()
            .state,
        "delivered"
    );
    let attached: Vec<_> = done
        .messages
        .iter()
        .filter(|m| !m.attachments.is_empty())
        .collect();
    assert_eq!(attached.len(), 1);
    assert_eq!(attached[0].attachments, vec![image]);
    e.stop().await;
}
