use bibi_core::*;
use bibi_server::{config::ServiceConfig, providers, runtime::Engine};
use serde_json::json;

fn fixture() -> (Engine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().unwrap();
    let engine = Engine::new(store, ServiceConfig::new(dir.path().into()));
    (engine, dir)
}
#[test]
fn codex_quota_preserves_unknown_windows_and_uses_account_windows() {
    let (engine, _dir) = fixture();
    providers::codex::persist_quota(&engine,&json!({"accountId":"test-account","rateLimits":{"primary":{"usedPercent":99}},"rateLimitsByLimitId":{
        "codex":{"primary":{"usedPercent":22,"windowDurationMins":300,"resetsAt":1800000000},"secondary":{"usedPercent":46,"windowDurationMins":10080,"resetsAt":1800010000}},
        "review":{"primary":null,"secondary":null}
    }})).unwrap();
    let snapshot = engine.store.snapshot().unwrap();
    let codex = snapshot
        .quotas
        .iter()
        .find(|q| q.id == "local:Codex")
        .unwrap();
    assert_eq!(codex.account, "test-account");
    assert_eq!(codex.windows[0].remaining_percent, Some(78.0));
    assert_eq!(codex.windows[0].resets_at, Some(1800000000000));
    assert_eq!(codex.windows[1].label, "주간");
    let unknown = snapshot
        .quotas
        .iter()
        .find(|q| q.id.ends_with(":review"))
        .unwrap();
    assert_eq!(unknown.status, "unknown");
    assert!(unknown.windows.is_empty());
}
#[tokio::test]
async fn ollama_http_stream_with_tool_roundtrip_completes_durably() {
    use axum::{Json, Router, extract::State, routing::post};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[derive(Clone)]
    struct Seen(Arc<AtomicUsize>);
    async fn chat(State(state): State<Seen>, Json(body): Json<serde_json::Value>) -> String {
        assert_eq!(body["stream"], true);
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["function"]["name"] == "guild_record")
        );
        let n = state.0.fetch_add(1, Ordering::SeqCst);
        if n == 2 {
            let messages = body["messages"].as_array().unwrap();
            assert!(
                messages
                    .iter()
                    .any(|m| m["role"] == "assistant" && m["content"] == "한글 응답 완료")
            );
            assert!(messages.iter().any(|m| m["role"] == "tool"));
            assert!(
                messages.last().unwrap()["content"]
                    .as_str()
                    .unwrap()
                    .contains("기억한 결과로 후속 답변")
            );
            return json!({"message":{"content":"후속 답변"},"done":true}).to_string();
        }

        if n == 0 {
            assert_eq!(body["messages"].as_array().unwrap().len(), 2);
            json!({"message":{"content":"","tool_calls":[{"function":{"name":"list_files","arguments":{"path":"."}}}]},"done":true,"prompt_eval_count":10,"eval_count":2}).to_string()
        } else {
            assert_eq!(
                body["messages"].as_array().unwrap().last().unwrap()["role"],
                "tool"
            );
            format!(
                "{}\n{}",
                json!({"message":{"content":"한글 응답"},"done":false}),
                json!({"message":{"content":" 완료"},"done":true,"prompt_eval_count":20,"eval_count":4})
            )
        }
    }
    let (mut engine, _dir) = fixture();
    let seen = Seen(Arc::new(AtomicUsize::new(0)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    engine.config.ollama_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/api/chat", post(chat))
                .with_state(seen.clone()),
        )
        .into_future(),
    );
    engine
        .store
        .add_project(Project {
            id: "p".into(),
            name: "p".into(),
            workspace: engine.config.data_dir.to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    engine.start().await.unwrap();
    let receipt = engine
        .store
        .submit(Submission {
            submission_id: "http".into(),
            project_key: "p".into(),
            work_id: None,
            title: None,
            question: "목록을 확인하고 답해라".into(),
            provider: Provider::Ollama,
            provider_id: None,
            model: "test-local".into(),
            host_id: "local".into(),
            role: "업무 조정".into(),
            mode: SubmitMode::Fresh,
            target_run_id: None,
            expected_turn_id: None,
            expected_context_revision: None,
            read_only: true,
        })
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if engine.store.run(&receipt.run_id).unwrap().state.terminal() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let detail = engine.store.detail(&receipt.run_id).unwrap();
    assert_eq!(detail.run.state, RunState::Completed);
    assert_eq!(detail.inbox[0].result, "한글 응답 완료");
    assert_eq!(detail.run.stats.input_tokens, Some(30));
    assert_eq!(detail.run.stats.output_tokens, Some(6));
    assert_eq!(seen.0.load(Ordering::SeqCst), 2);
    let follow = engine
        .store
        .submit(Submission {
            submission_id: "http-follow".into(),
            project_key: "p".into(),
            work_id: Some(detail.run.work_id.clone()),
            title: None,
            question: "기억한 결과로 후속 답변".into(),
            provider: Provider::Ollama,
            provider_id: None,
            model: "test-local".into(),
            host_id: "local".into(),
            role: "업무 조정".into(),
            mode: SubmitMode::Continue,
            target_run_id: Some(detail.run.id.clone()),
            expected_turn_id: detail.run.turn_id.clone(),
            expected_context_revision: Some(1),
            read_only: true,
        })
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !engine.store.run(&follow.run_id).unwrap().state.terminal() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let followed = engine.store.detail(&follow.run_id).unwrap();
    assert_eq!(followed.run.state, RunState::Completed);
    assert_eq!(followed.run.session_key, detail.run.session_key);
    assert_eq!(seen.0.load(Ordering::SeqCst), 3);

    engine.stop().await;
    server.abort();
}
#[tokio::test]
#[ignore = "Requires the local Codex installation and the explicit BiBi smoke workspace; never calls a model"]
async fn live_codex_external_history_is_read_only() {
    let (engine, _dir) = fixture();
    let workspace = std::env::var("BIBI_SMOKE_WORKSPACE").expect("explicit test workspace");
    engine
        .store
        .add_project(Project {
            id: "external".into(),
            name: "external".into(),
            workspace,
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    let result = providers::codex::discover(&engine, "external")
        .await
        .unwrap();
    assert!(result["imported"].as_u64().unwrap() > 0, "{result}");
    assert_eq!(result["errors"].as_array().unwrap().len(), 0, "{result}");
    let snapshot = engine.store.snapshot().unwrap();
    assert!(snapshot.runs.iter().all(|r| r.origin == Origin::External
        && !r.capabilities.send_to_active.supported
        && !r.capabilities.interrupt.supported));
    assert!(snapshot.runs.iter().any(|r| {
        engine
            .store
            .detail(&r.id)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "BIBI_CONNECTED")
    }));
}

#[test]
fn ollama_local_and_cloud_models_do_not_share_unlimited_status() {
    let quotas = providers::ollama::model_quotas(
        &[
            json!({"name":"gemma4"}),
            json!({"name":"big:cloud","remote_host":"cloud.example"}),
        ],
        true,
    );
    assert_eq!(quotas[0].status, "unlimited");
    assert_eq!(quotas[1].status, "unknown");
    assert_ne!(quotas[0].id, quotas[1].id);
    assert_eq!(
        providers::ollama::model_quotas(&[json!({"name":"gemma4"})], false)[0].status,
        "unknown"
    );
}
