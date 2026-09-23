use bibi_core::*;
use bibi_server::{config::ServiceConfig, providers, runtime::Engine};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn oauth_usage_and_runtime_usage_use_their_respective_units() {
    let windows = providers::claude_usage::windows(
        &json!({"five_hour":{"utilization":46,"resets_at":"2026-09-23T12:00:00Z"},"seven_day":{"utilization":20},"seven_day_sonnet":null}),
    );
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].remaining_percent, Some(54.0));
    assert!(windows[0].resets_at.is_some());
    assert_eq!(
        providers::claude_usage::access_token(&json!({"claudeAiOauth":{"accessToken":"fixture"}}))
            .as_deref(),
        Some("fixture")
    );
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Store::memory().unwrap(),
        ServiceConfig::new(d.path().into()),
    );
    providers::claude::persist_quota(
        &e,
        &json!({"rateLimitType":"five_hour","status":"allowed","utilization":0.46}),
    )
    .unwrap();
    providers::claude::persist_quota(&e, &json!({"rateLimitType":"five_hour","status":"allowed"}))
        .unwrap();
    assert_eq!(
        e.store.quotas().unwrap()[0].windows[0].remaining_percent,
        Some(54.0)
    );
}
#[test]
fn native_command_catalog_accepts_string_and_object_formats() {
    let commands = providers::claude::commands(
        &json!(["compact",{"name":"/model","description":"모델","argumentHint":"name"},"compact",null]),
    );
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[1].name, "model");
    assert_eq!(commands[1].argument_hint, "name");
}
#[tokio::test]
async fn native_session_path_uses_config_directory_and_rejects_path_injection() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("projects").join("workspace");
    tokio::fs::create_dir_all(&path).await.unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let file = path.join(format!("{id}.jsonl"));
    tokio::fs::write(&file, b"{}").await.unwrap();
    assert_eq!(
        providers::claude_usage::session_file_in(root.path(), &id).await,
        Some(file.to_string_lossy().into())
    );
    assert!(
        providers::claude_usage::session_file_in(root.path(), "../../secret")
            .await
            .is_none()
    );
}
#[test]
fn command_placeholders_never_expand_user_text_as_another_placeholder() {
    assert_eq!(
        providers::command::expand_argument(
            "--prompt={prompt};model={model}",
            &[("prompt", "keep {model} literal"), ("model", "m1")]
        ),
        "--prompt=keep {model} literal;model=m1"
    );
}
#[test]
fn sse_decoder_handles_unicode_split_at_every_byte() {
    let mut parser = providers::openai::Sse::default();
    let mut frames = vec![];
    for b in "data: {\"text\":\"한글\"}\r\n\r\ndata: [DONE]\n\n".as_bytes() {
        frames.extend(parser.push(&[*b]).unwrap());
    }
    assert_eq!(frames, vec!["{\"text\":\"한글\"}", "[DONE]"]);
}

#[tokio::test]
async fn compatible_api_stream_preserves_history_and_actual_model() {
    use axum::{Json, Router, http::HeaderMap, response::IntoResponse, routing::post};
    async fn answer(headers: HeaderMap, Json(body): Json<Value>) -> impl IntoResponse {
        assert_eq!(headers["authorization"], "Bearer fixture-key");
        let messages = body["messages"].as_array().unwrap();
        if messages.len() > 2 {
            assert!(
                messages
                    .iter()
                    .any(|m| m["role"] == "assistant" && m["content"] == "**한글** 답변")
            );
        }
        let data = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"model":"actual-model","choices":[{"delta":{"content":"**한글** 답변"},"finish_reason":null}]}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3}})
        );
        ([("content-type", "text/event-stream")], data)
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new().route("/v1/chat/completions", post(answer)),
        )
        .into_future(),
    );
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Store::memory().unwrap(),
        ServiceConfig::new(d.path().into()),
    );
    e.store
        .add_project(Project {
            id: "p".into(),
            name: "p".into(),
            workspace: d.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    let provider: ProviderConfig = serde_json::from_value(
        json!({"id":"api","name":"내 API","adapter":"open_ai","endpoint":endpoint}),
    )
    .unwrap();
    e.store
        .save_provider(provider, Some("fixture-key".into()))
        .unwrap();
    e.start().await.unwrap();
    let mut request:Submission=serde_json::from_value(json!({"submission_id":"first","project_key":"p","question":"질문","provider":"open_ai","provider_id":"api","model":"alias","host_id":"local","role":"coordinator","mode":"fresh","read_only":true})).unwrap();
    let receipt = e.store.submit(request.clone()).unwrap();
    let a = wait(&e, &receipt.run_id).await;
    assert_eq!(a.state, RunState::Completed, "{:?}", a.error);
    assert_eq!(a.model, "actual-model");
    assert_eq!(a.stats.output_tokens, Some(3));
    request.submission_id = "follow".into();
    request.mode = SubmitMode::Continue;
    request.model = a.model.clone();
    request.target_run_id = Some(a.id.clone());
    request.expected_turn_id = a.turn_id.clone();
    request.expected_context_revision = Some(1);
    let next = e.store.submit(request).unwrap();
    let b = wait(&e, &next.run_id).await;
    assert_eq!(b.state, RunState::Completed, "{:?}", b.error);
    assert_eq!(a.session_key, b.session_key);
    e.stop().await;
    server.abort();
}
async fn wait(e: &Engine, id: &str) -> Run {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let r = e.store.run(id).unwrap();
            if r.state.terminal() {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn custom_command_preserves_conversation_and_interrupts_after_stdout_closes() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Store::memory().unwrap(),
        ServiceConfig::new(d.path().into()),
    );
    e.store
        .add_project(Project {
            id: "p".into(),
            name: "p".into(),
            workspace: d.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    let provider:ProviderConfig=serde_json::from_value(json!({"id":"cli","name":"사용자 명령","adapter":"command","command":"node","args":[format!("{}/tests/fixtures/command.mjs",env!("CARGO_MANIFEST_DIR"))]})).unwrap();
    e.store.save_provider(provider, None).unwrap();
    e.start().await.unwrap();
    let mut request:Submission=serde_json::from_value(json!({"submission_id":"cmd-first","project_key":"p","question":"literal {model}","provider":"command","provider_id":"cli","model":"fixture","host_id":"local","role":"coordinator","mode":"fresh","read_only":false})).unwrap();
    let first = e.store.submit(request.clone()).unwrap();
    let a = wait(&e, &first.run_id).await;
    assert_eq!(a.state, RunState::Completed, "{:?}", a.error);
    let text = e
        .store
        .detail(&a.id)
        .unwrap()
        .messages
        .into_iter()
        .find(|m| m.role == "assistant")
        .unwrap()
        .text;
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["prompt"], "literal {model}");
    assert_eq!(value["turns"], 1);
    request.submission_id = "cmd-follow".into();
    request.mode = SubmitMode::Continue;
    request.target_run_id = Some(a.id);
    request.expected_turn_id = a.turn_id;
    request.expected_context_revision = Some(1);
    let second = e.store.submit(request.clone()).unwrap();
    let b = wait(&e, &second.run_id).await;
    assert_eq!(b.state, RunState::Completed, "{:?}", b.error);
    let text = e
        .store
        .detail(&b.id)
        .unwrap()
        .messages
        .into_iter()
        .find(|m| m.role == "assistant")
        .unwrap()
        .text;
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["turns"], 2);
    assert_eq!(value["previous"], true);
    request.submission_id = "cmd-interrupt".into();
    request.question = "HOLD_CLOSED_STDOUT".into();
    request.target_run_id = Some(b.id);
    request.expected_turn_id = b.turn_id;
    let third = e.store.submit(request).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while e.store.run(&third.run_id).unwrap().turn_id.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    e.interrupt(&third.run_id).await.unwrap();
    assert_eq!(wait(&e, &third.run_id).await.state, RunState::Interrupted);
    e.stop().await;
}
