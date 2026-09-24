use bibi_core::*;
use bibi_server::{config::ServiceConfig, providers, runtime::Engine};
#[cfg(unix)]
use serde_json::Value;
use serde_json::json;
#[cfg(unix)]
use std::time::Duration;

fn request(key: &str, provider: Provider) -> Submission {
    Submission {
        submission_id: key.into(),
        project_key: "p".into(),
        work_id: None,
        title: None,
        question: key.into(),
        provider,
        provider_id: None,
        model: String::new(),
        host_id: "local".into(),
        role: "업무 조정".into(),
        mode: SubmitMode::Fresh,
        target_run_id: None,
        expected_turn_id: None,
        expected_context_revision: None,
        read_only: false,
    }
}
#[cfg(unix)]
fn follow(engine: &Engine, run_id: &str, key: &str) -> Submission {
    let run = engine.store.run(run_id).unwrap();
    let mut next = request(key, run.provider);
    next.model = run.model;
    next.mode = SubmitMode::Continue;
    next.target_run_id = Some(run.id);
    next.expected_turn_id = run.turn_id;
    next.expected_context_revision = Some(1);
    next
}
fn setup() -> (Engine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().unwrap();
    store
        .add_project(Project {
            id: "p".into(),
            name: "fixture".into(),
            workspace: dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    (
        Engine::new(store, ServiceConfig::new(dir.path().into())),
        dir,
    )
}
#[cfg(unix)]
async fn finished(engine: &Engine, receipt: &Receipt, approve: bool) -> RunDetail {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let detail = engine.store.detail(&receipt.run_id).unwrap();
            if detail.run.state.terminal() {
                return detail;
            }
            if approve {
                for a in detail.approvals.iter().filter(|a| a.state == "pending") {
                    engine
                        .respond(&a.id, json!({"decision":"accept"}))
                        .await
                        .unwrap();
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fixture timed out")
}

#[cfg(unix)]
#[tokio::test]
async fn claude_reuses_live_process_restores_after_restart_and_tracks_tools_agents_and_quota() {
    let (mut engine, _dir) = setup();
    engine.config.claude_command =
        format!("{}/tests/fixtures/claude.py", env!("CARGO_MANIFEST_DIR"));
    engine.start().await.unwrap();
    providers::claude::refresh(&engine).await.unwrap();
    let first = engine
        .store
        .submit(request("FIRST_FIXTURE", Provider::Claude))
        .unwrap();
    let a = finished(&engine, &first, true).await;
    assert_eq!(a.run.state, RunState::Completed, "{:?}", a.run.error);
    let text = &a
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .unwrap()
        .text;
    let first_result: Value = serde_json::from_str(text).unwrap();
    assert_eq!(first_result["turns"], 1);
    let children = engine
        .store
        .work_runs(&first.work_id)
        .unwrap()
        .into_iter()
        .filter(|r| r.agent_kind == "subagent")
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].state, RunState::Completed);
    assert_eq!(children[0].model, "fixture-child-model");
    assert!(children[0].runtime.session_file.is_none());
    assert_eq!(
        children[0].parent_session_id.as_deref(),
        Some(a.run.session_id.as_str())
    );
    assert!(
        !a.messages
            .iter()
            .any(|m| m.text.contains("서브에이전트의 별도 답변"))
    );
    assert!(
        engine
            .store
            .detail(&children[0].id)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "서브에이전트의 별도 답변")
    );
    assert!(
        a.messages
            .iter()
            .any(|m| m.text.starts_with("bibi_list_files"))
    );
    assert_eq!(a.approvals[0].state, "delivered");
    assert_eq!(a.run.model, "fixture-claude");
    assert!(a.run.runtime.commands.iter().any(|c| c.name == "compact"));
    assert_eq!(a.run.stats.input_tokens, Some(40));
    let quota = engine
        .store
        .quotas()
        .unwrap()
        .into_iter()
        .find(|q| q.provider == Provider::Claude)
        .unwrap();
    assert_eq!(quota.windows[0].remaining_percent, Some(54.0));
    let mut next = follow(&engine, &a.run.id, "SECOND_FIXTURE");
    next.model = "changed-claude".into();
    let second = engine.store.submit(next).unwrap();
    let b = finished(&engine, &second, false).await;
    assert_eq!(b.run.state, RunState::Completed, "{:?}", b.run.error);
    assert_eq!(a.run.session_key, b.run.session_key);
    assert_eq!(a.run.session_id, b.run.session_id);
    let result: Value = serde_json::from_str(
        &b.messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(result["turns"], 2);
    assert_eq!(result["model"], "changed-claude");
    assert_eq!(result["model_changes"], 1);
    assert_eq!(b.run.model, "changed-claude");
    assert_eq!(result["pid"], first_result["pid"]);
    assert!(b.conversation.len() > b.messages.len());
    engine.stop().await;
    let engine = Engine::new(engine.store.clone(), engine.config.clone());
    engine.start().await.unwrap();
    let mut next = follow(&engine, &b.run.id, "THIRD_FIXTURE");
    next.model = "restored-claude".into();
    let third = engine.store.submit(next).unwrap();
    let c = finished(&engine, &third, false).await;
    assert_eq!(c.run.state, RunState::Completed, "{:?}", c.run.error);
    assert_eq!(c.run.session_key, a.run.session_key);
    let result: Value = serde_json::from_str(
        &c.messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(result["turns"], 3);
    assert_eq!(result["model"], "restored-claude");
    assert_eq!(c.run.model, "restored-claude");
    assert_eq!(result["resumed"], true);
    assert_ne!(result["pid"], first_result["pid"]);
    let fourth = engine
        .store
        .submit(follow(&engine, &c.run.id, "INTERRUPT_FIXTURE"))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if engine.store.run(&fourth.run_id).unwrap().turn_id.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    engine.interrupt(&fourth.run_id).await.unwrap();
    assert_eq!(
        finished(&engine, &fourth, false).await.run.state,
        RunState::Interrupted
    );
    let stopped = engine.store.run(&fourth.run_id).unwrap();
    let slash = engine
        .store
        .submit(follow(&engine, &stopped.id, "/compact"))
        .unwrap();
    let slash_detail = finished(&engine, &slash, false).await;
    assert_eq!(
        slash_detail.run.state,
        RunState::Completed,
        "{:?}",
        slash_detail.run.error
    );
    let answer = slash_detail
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&answer.text).unwrap()["last"],
        "/compact"
    );
    let unsupported = engine
        .store
        .submit(follow(&engine, &slash.run_id, "/not-a-command"))
        .unwrap();
    let failed = finished(&engine, &unsupported, false).await;
    assert_eq!(failed.run.state, RunState::Failed);
    assert!(failed.run.turn_id.is_none());
    engine.stop().await;
}

#[tokio::test]
async fn native_codex_children_are_separate_and_duplicate_events_preserve_send_time() {
    let (engine, _dir) = setup();
    engine.start().await.unwrap();
    let receipt = engine
        .store
        .submit(request("ROOT", Provider::Codex))
        .unwrap();
    // Claim directly before the scheduler's first 200 ms cycle; no model process is started.
    let root = engine.store.claim_next("local").unwrap().unwrap();
    engine
        .store
        .delivered(&root.id, "root-native", "turn")
        .unwrap();
    let item = json!({"threadId":"root-native","item":{"type":"collabAgentToolCall","id":"spawn-1","tool":"spawnAgent","senderThreadId":"root-native","receiverThreadIds":["child-native"],"prompt":"독립 검토","agentsStates":{"child-native":{"status":"running","message":null}}}});
    providers::codex::observe_agents(&engine, &root, "root-native", "item/completed", &item)
        .unwrap();
    let edge = engine
        .store
        .snapshot()
        .unwrap()
        .transmissions
        .into_iter()
        .find(|e| e.to_run_id != root.id)
        .unwrap();
    providers::codex::observe_agents(&engine, &root, "root-native", "item/completed", &item)
        .unwrap();
    let children = engine
        .store
        .work_runs(&receipt.work_id)
        .unwrap()
        .into_iter()
        .filter(|r| r.agent_kind == "subagent")
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 1);
    providers::codex::observe_agents(
        &engine,
        &root,
        "root-native",
        "item/agentMessage/delta",
        &json!({"threadId":"child-native","itemId":"child-answer","delta":"전문가 답변"}),
    )
    .unwrap();
    providers::codex::observe_agents(
        &engine,
        &root,
        "root-native",
        "turn/completed",
        &json!({"threadId":"child-native","turn":{"id":"child-turn","status":"completed"}}),
    )
    .unwrap();
    let child = engine.store.detail(&children[0].id).unwrap();
    assert_eq!(child.run.state, RunState::Completed);
    assert!(child.messages.iter().any(|m| m.text == "전문가 답변"));
    assert!(
        !engine
            .store
            .detail(&root.id)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "전문가 답변")
    );
    assert_eq!(
        engine
            .store
            .snapshot()
            .unwrap()
            .transmissions
            .into_iter()
            .find(|e| e.id == edge.id)
            .unwrap()
            .sent_at,
        edge.sent_at
    );
    engine.store.fail(&root.id, "fixture done", true).unwrap();
    engine.stop().await;
}

#[tokio::test]
async fn claude_background_completion_patch_does_not_mix_bash_tasks_or_thinking() {
    let (engine, _dir) = setup();
    engine.start().await.unwrap();
    let receipt = engine
        .store
        .submit(request("ROOT", Provider::Claude))
        .unwrap();
    let run = engine.store.claim_next("local").unwrap().unwrap();
    let mut events = providers::claude::Events::default();
    events.consume(&engine,&run,&json!({"type":"system","subtype":"task_started","task_id":"shell","task_type":"local_bash","description":"command"})).unwrap();
    events.consume(&engine,&run,&json!({"type":"system","subtype":"task_started","task_id":"agent","task_type":"local_agent","tool_use_id":"tool","description":"background expert"})).unwrap();
    events.consume(&engine,&run,&json!({"type":"stream_event","parent_tool_use_id":"tool","event":{"delta":{"type":"thinking_delta","thinking":"PRIVATE"}}})).unwrap();
    events.consume(&engine,&run,&json!({"type":"system","subtype":"task_updated","task_id":"agent","patch":{"status":"failed"}})).unwrap();
    let children = engine
        .store
        .work_runs(&receipt.work_id)
        .unwrap()
        .into_iter()
        .filter(|r| r.agent_kind == "subagent")
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].state, RunState::Failed);
    assert!(
        !engine
            .store
            .detail(&children[0].id)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text.contains("PRIVATE"))
    );
    engine.store.fail(&run.id, "fixture done", true).unwrap();
    engine.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn codex_continues_live_thread_and_restores_same_thread_after_restart() {
    let (mut engine, _dir) = setup();
    engine.config.codex_command = format!("{}/tests/fixtures/codex.py", env!("CARGO_MANIFEST_DIR"));
    engine.start().await.unwrap();
    let first = engine
        .store
        .submit(request("FIRST", Provider::Codex))
        .unwrap();
    let a = finished(&engine, &first, false).await;
    assert_eq!(a.run.state, RunState::Completed, "{:?}", a.run.error);
    let first_result: Value = serde_json::from_str(
        &a.messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(a.children.len(), 1);
    assert_eq!(a.children[0].run.state, RunState::Completed);
    assert!(
        a.children[0]
            .messages
            .iter()
            .any(|m| m.text == "late child answer")
    );
    let mut next = follow(&engine, &a.run.id, "SECOND");
    next.model = "changed-codex".into();
    let second = engine.store.submit(next).unwrap();
    let b = finished(&engine, &second, false).await;
    assert_eq!(b.run.state, RunState::Completed, "{:?}", b.run.error);
    assert_eq!(a.run.session_key, b.run.session_key);
    let second_result: Value = serde_json::from_str(
        &b.messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(second_result["turns"], 2);
    assert_eq!(second_result["model"], "changed-codex");
    assert_eq!(a.run.session_id, b.run.session_id);
    assert_eq!(second_result["pid"], first_result["pid"]);
    engine.stop().await;
    let engine = Engine::new(engine.store.clone(), engine.config.clone());
    engine.start().await.unwrap();
    let mut next = follow(&engine, &b.run.id, "THIRD");
    next.model = "restored-codex".into();
    let third = engine.store.submit(next).unwrap();
    let c = finished(&engine, &third, false).await;
    assert_eq!(c.run.state, RunState::Completed, "{:?}", c.run.error);
    assert_eq!(c.run.session_key, a.run.session_key);
    let result: Value = serde_json::from_str(
        &c.messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(result["turns"], 3);
    assert_eq!(result["model"], "restored-codex");
    assert_eq!(result["resumed"], true);
    engine.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn claude_model_change_rejection_does_not_send_the_question() {
    let (mut engine, dir) = setup();
    engine.config.claude_command =
        format!("{}/tests/fixtures/claude.py", env!("CARGO_MANIFEST_DIR"));
    engine.start().await.unwrap();
    let first = engine
        .store
        .submit(request("FIRST", Provider::Claude))
        .unwrap();
    let a = finished(&engine, &first, true).await;
    assert_eq!(a.run.state, RunState::Completed);
    let path = dir.path().join(format!(
        "{}.fixture.json",
        a.run.session_key.as_ref().unwrap()
    ));
    let before = std::fs::read(&path).unwrap();
    let mut next = follow(&engine, &a.run.id, "MUST_NOT_SEND");
    next.model = "reject-model".into();
    let second = engine.store.submit(next).unwrap();
    let b = finished(&engine, &second, false).await;
    assert_eq!(b.run.state, RunState::Failed);
    assert!(
        b.run
            .error
            .as_deref()
            .unwrap()
            .contains("fixture model rejected")
    );
    assert!(b.run.turn_id.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    engine.stop().await;
}
