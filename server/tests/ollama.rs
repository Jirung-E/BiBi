use axum::{Json, Router, extract::State, routing::post};
use bibi_core::*;
use bibi_server::{config::ServiceConfig, runtime::Engine};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct Responses {
    replies: Vec<String>,
    requests: Arc<Mutex<Vec<Value>>>,
}

async fn chat(State(state): State<Responses>, Json(body): Json<Value>) -> String {
    let mut requests = state.requests.lock().unwrap();
    let index = requests.len();
    requests.push(body);
    state
        .replies
        .get(index)
        .cloned()
        .unwrap_or_else(|| json!({"error":"unexpected extra request"}).to_string())
}

struct Fixture {
    engine: Engine,
    requests: Arc<Mutex<Vec<Value>>>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    _dir: tempfile::TempDir,
}

impl Fixture {
    async fn start(replies: Vec<Vec<Value>>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(
            Store::memory().unwrap(),
            ServiceConfig::new(dir.path().into()),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        engine.config.ollama_url = format!("http://{}", listener.local_addr().unwrap());
        let responses = Responses {
            replies: replies
                .into_iter()
                .map(|frames| {
                    frames
                        .iter()
                        .map(Value::to_string)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect(),
            requests: Arc::default(),
        };
        let requests = responses.requests.clone();
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new()
                    .route("/api/chat", post(chat))
                    .with_state(responses),
            )
            .into_future(),
        );
        engine
            .store
            .add_project(Project {
                id: "p".into(),
                name: "p".into(),
                workspace: dir.path().to_string_lossy().into(),
                guild_path: None,
                constraints: vec![],
            })
            .unwrap();
        engine.store.save_provider(serde_json::from_value(json!({
            "id":"local-ollama", "name":"Local", "adapter":"ollama", "endpoint":engine.config.ollama_url
        })).unwrap(), None).unwrap();
        engine.start().await.unwrap();
        Self {
            engine,
            requests,
            server,
            _dir: dir,
        }
    }

    async fn ask(&self, question: &str, previous: Option<&Run>) -> RunDetail {
        let receipt = self
            .engine
            .store
            .submit(Submission {
                submission_id: id("submission"),
                project_key: "p".into(),
                work_id: previous.map(|r| r.work_id.clone()),
                title: None,
                question: question.into(),
                provider: Provider::Ollama,
                provider_id: Some("local-ollama".into()),
                model: "gemma4:e4b".into(),
                host_id: "local".into(),
                role: "업무 조정".into(),
                mode: if previous.is_some() {
                    SubmitMode::Continue
                } else {
                    SubmitMode::Fresh
                },
                target_run_id: previous.map(|r| r.id.clone()),
                expected_turn_id: previous.and_then(|r| r.turn_id.clone()),
                expected_context_revision: previous.map(|r| r.context_revision),
                read_only: true,
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !self
                .engine
                .store
                .run(&receipt.run_id)
                .unwrap()
                .state
                .terminal()
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        self.engine.store.detail(&receipt.run_id).unwrap()
    }

    async fn stop(self) {
        self.engine.stop().await;
        self.server.abort();
    }
}

#[tokio::test]
async fn empty_final_responses_fail_without_blank_messages_and_allow_continuation() {
    for (content, thinking, reason) in [
        ("", "", "stop"),
        ("", "추론만 생성됨", "stop"),
        (" \n\t", "추론만 생성됨", "length"),
    ] {
        let fixture = Fixture::start(vec![
            vec![json!({"message":{"content":content,"thinking":thinking},"done":true,"done_reason":reason,"prompt_eval_count":10,"eval_count":2})],
            vec![json!({"message":{"content":"후속 답변"},"done":true})],
        ]).await;
        let failed = fixture.ask("처음 질문", None).await;
        assert_eq!(
            failed.run.state,
            RunState::Failed,
            "content={content:?}, thinking={thinking:?}"
        );
        assert!(failed.run.error.as_deref().unwrap().contains("최종 답변"));
        if reason == "length" {
            assert!(failed.run.error.as_deref().unwrap().contains("길이 한도"));
        }
        assert!(failed.inbox.is_empty());
        assert!(!failed.messages.iter().any(|m| m.role == "assistant"));
        assert_eq!(failed.run.stats.input_tokens, Some(10));
        assert_eq!(failed.run.stats.output_tokens, Some(2));
        assert!(failed.run.session_key.is_some());
        let history: Vec<Value> = fixture
            .engine
            .store
            .setting(&format!("ollama:history:{}", failed.run.id))
            .unwrap()
            .unwrap();
        assert_eq!(history.len(), 2);
        let followed = fixture.ask("다시 답변해줘", Some(&failed.run)).await;
        assert_eq!(followed.run.state, RunState::Completed);
        assert_eq!(followed.run.session_id, failed.run.session_id);
        assert_eq!(followed.run.session_key, failed.run.session_key);
        assert_eq!(followed.inbox[0].result, "후속 답변");
        let requests = fixture.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|r| r.get("options").is_none() && r.get("think").is_none())
        );
        assert_eq!(requests[0]["messages"][1], requests[1]["messages"][1]);
        assert_eq!(requests[1]["messages"].as_array().unwrap().len(), 3);
        fixture.stop().await;
    }
}

#[tokio::test]
async fn empty_reply_after_a_tool_preserves_tool_history_for_followup() {
    let fixture = Fixture::start(vec![
        vec![json!({"message":{"thinking":"작업 경로를 확인합니다.","content":"확인하겠습니다.","tool_calls":[{"function":{"name":"list_files","arguments":{"path":"."}}}]},"done":true})],
        vec![json!({"message":{"thinking":"답변을 준비합니다.","content":""},"done":true,"done_reason":"stop"})],
        vec![json!({"message":{"content":"저장한 목록으로 답변합니다."},"done":true})],
    ]).await;
    let failed = fixture.ask("파일 목록 확인", None).await;
    assert_eq!(failed.run.state, RunState::Failed);
    assert!(failed.inbox.is_empty());
    assert_eq!(
        failed.messages.iter().filter(|m| m.role == "tool").count(),
        1
    );
    let followed = fixture
        .ask("확인한 결과를 답변해줘", Some(&failed.run))
        .await;
    assert_eq!(followed.run.state, RunState::Completed);
    assert_eq!(followed.run.session_key, failed.run.session_key);
    let requests = fixture.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 3);
    let history = requests[2]["messages"].as_array().unwrap();
    assert_eq!(history.len(), 5);
    assert_eq!(history[2]["thinking"], "작업 경로를 확인합니다.");
    assert_eq!(history[2]["content"], "확인하겠습니다.");
    assert_eq!(
        history[2]["tool_calls"][0]["function"]["name"],
        "list_files"
    );
    assert_eq!(history[3]["role"], "tool");
    assert_eq!(history[3]["tool_name"], "list_files");
    assert_eq!(requests[1]["messages"][3], history[3]);
    assert!(
        !history
            .iter()
            .any(|m| m["thinking"] == "답변을 준비합니다.")
    );
    fixture.stop().await;
}

#[tokio::test]
async fn leading_whitespace_is_preserved_when_real_content_arrives() {
    let fixture = Fixture::start(vec![vec![
        json!({"message":{"content":" \n"},"done":false}),
        json!({"message":{"thinking":"확인 중","content":"\t"},"done":false}),
        json!({"message":{"content":"정상 답변"},"done":false}),
        json!({"message":{"content":"\n "},"done":true}),
    ]])
    .await;
    let detail = fixture.ask("답변해줘", None).await;
    assert_eq!(detail.run.state, RunState::Completed);
    assert_eq!(detail.inbox[0].result, " \n\t정상 답변\n ");
    let answers: Vec<_> = detail
        .messages
        .iter()
        .filter(|m| m.role == "assistant")
        .collect();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].text, detail.inbox[0].result);
    fixture.stop().await;
}

#[tokio::test]
async fn legacy_blank_completions_are_skipped_but_empty_tool_messages_are_kept() {
    let fixture = Fixture::start(vec![
        vec![json!({"message":{"content":""},"done":true})],
        vec![json!({"message":{"content":"이어진 답변"},"done":true})],
    ])
    .await;
    let previous = fixture.ask("이전 질문", None).await;
    let key = format!("ollama:history:{}", previous.run.id);
    let mut history: Vec<Value> = fixture.engine.store.setting(&key).unwrap().unwrap();
    let call = json!({"role":"assistant","content":"","tool_calls":[{"function":{"name":"list_files","arguments":{"path":"."}}}]});
    history.extend([
        call.clone(),
        json!({"role":"tool","tool_name":"list_files","content":"{\"files\":[]}"}),
        json!({"role":"assistant","content":" \n\t"}),
    ]);
    fixture.engine.store.set_setting(&key, &history).unwrap();
    let followed = fixture.ask("다시 답해줘", Some(&previous.run)).await;
    assert_eq!(followed.run.state, RunState::Completed);
    let requests = fixture.requests.lock().unwrap().clone();
    let messages = requests[1]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[2], call);
    assert_eq!(messages[3]["role"], "tool");
    assert_eq!(messages[4]["role"], "user");
    fixture.stop().await;
}

#[tokio::test]
async fn changing_generation_settings_recovers_in_the_same_conversation_and_tool_roundtrip() {
    let fixture = Fixture::start(vec![
        vec![json!({"message":{"thinking":"아직 추론 중","content":""},"done":true,"done_reason":"length","eval_count":16})],
        vec![json!({"message":{"content":"","tool_calls":[{"function":{"name":"list_files","arguments":{"path":"."}}}]},"done":true})],
        vec![json!({"message":{"content":"최종 답변"},"done":true,"done_reason":"stop"})],
    ]).await;
    let mut p = fixture.engine.store.provider("local-ollama").unwrap();
    p.ollama = Some(OllamaOptions {
        num_predict: Some(16),
        num_ctx: Some(4096),
        ..Default::default()
    });
    fixture.engine.store.save_provider(p.clone(), None).unwrap();
    let failed = fixture.ask("목록을 확인하고 답하세요", None).await;
    assert_eq!(failed.run.state, RunState::Failed);
    assert!(failed.run.error.as_deref().unwrap().contains("생성 16토큰"));
    assert!(failed.run.error.as_deref().unwrap().contains("생성 설정"));
    assert!(failed.inbox.is_empty());
    p.ollama = Some(OllamaOptions {
        think: Some(false),
        num_predict: Some(2048),
        num_ctx: Some(8192),
    });
    fixture.engine.store.save_provider(p, None).unwrap();
    let recovered = fixture
        .ask("설정을 변경했습니다. 다시 답하세요.", Some(&failed.run))
        .await;
    assert_eq!(recovered.run.state, RunState::Completed);
    assert_eq!(recovered.run.session_id, failed.run.session_id);
    assert_eq!(recovered.run.session_key, failed.run.session_key);
    assert_eq!(recovered.inbox[0].result, "최종 답변");
    let requests = fixture.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].get("think").is_none());
    assert_eq!(
        requests[0]["options"],
        json!({"num_predict":16,"num_ctx":4096})
    );
    for body in &requests[1..] {
        assert_eq!(body["think"], false);
        assert_eq!(body["options"], json!({"num_predict":2048,"num_ctx":8192}));
        assert_eq!(body["messages"][1], requests[0]["messages"][1]);
    }
    assert_eq!(
        requests[2]["messages"].as_array().unwrap().last().unwrap()["role"],
        "tool"
    );
    fixture.stop().await;
}

#[tokio::test]
async fn truncated_answers_keep_partial_text_but_never_complete_or_execute_tools() {
    let fixture=Fixture::start(vec![
        vec![json!({"message":{"content":"아직 작성 중인 답변", "tool_calls":[{"function":{"name":"list_files","arguments":{"path":"."}}}]},"done":true,"done_reason":"length","eval_count":12})],
        vec![json!({"message":{"content":"완성된 답변"},"done":true,"done_reason":"stop"})],
    ]).await;
    let failed = fixture.ask("답변해줘", None).await;
    assert_eq!(failed.run.state, RunState::Failed);
    assert!(
        failed
            .run
            .error
            .as_deref()
            .unwrap()
            .contains("중간에 끊겼습니다")
    );
    assert!(failed.inbox.is_empty());
    assert!(
        failed
            .messages
            .iter()
            .any(|m| m.role == "assistant" && m.text == "아직 작성 중인 답변")
    );
    assert!(!failed.messages.iter().any(|m| m.role == "tool"));
    let recovered = fixture.ask("이어서 완성해줘", Some(&failed.run)).await;
    assert_eq!(recovered.run.state, RunState::Completed);
    assert_eq!(recovered.run.session_id, failed.run.session_id);
    let requests = fixture.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let previous = &requests[1]["messages"][2];
    assert_eq!(previous["content"], "아직 작성 중인 답변");
    assert!(previous.get("tool_calls").is_none());
    fixture.stop().await;
}
