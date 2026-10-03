use bibi_core::*;
use bibi_server::{config::ServiceConfig, providers, runtime::Engine};
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

fn claude_fixture(engine: &mut Engine) {
    engine.config.claude_command = "node".into();
    engine.config.claude_args = vec![format!(
        "{}/tests/fixtures/claude.mjs",
        env!("CARGO_MANIFEST_DIR")
    )];
}

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
        approval_mode: None,
    }
}
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

#[tokio::test]
async fn claude_reuses_live_process_restores_after_restart_and_tracks_tools_agents_and_quota() {
    let (mut engine, _dir) = setup();
    claude_fixture(&mut engine);
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
        &engine, &root, "root-native", "item/started",
        &json!({"threadId":"child-native","item":{"type":"agentMessage","id":"child-answer","phase":"final_answer","text":""}}),
    ).unwrap();
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
    assert!(
        child
            .messages
            .iter()
            .any(|m| m.text == "전문가 답변" && m.phase.as_deref() == Some("final_answer"))
    );
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

#[tokio::test]
async fn claude_model_change_rejection_does_not_send_the_question() {
    let (mut engine, dir) = setup();
    claude_fixture(&mut engine);
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

#[tokio::test]
async fn claude_messages_reuse_agents_and_deliver_late_approvals_without_another_user_turn() {
    let (mut engine, _dir) = setup();
    claude_fixture(&mut engine);
    engine.start().await.unwrap();
    let first = engine
        .store
        .submit(request("FIRST", Provider::Claude))
        .unwrap();
    let a = finished(&engine, &first, true).await;
    let child = a.children[0].run.id.clone();
    assert_eq!(
        engine
            .store
            .subagent_by_native(&a.run.id, "native-expert")
            .unwrap()
            .unwrap()
            .id,
        child
    );
    let mut previous = a.run.id;
    for turn in 2..=3 {
        let next = engine
            .store
            .submit(follow(
                &engine,
                &previous,
                &format!("MESSAGE_FIXTURE_{turn}"),
            ))
            .unwrap();
        let approval = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let detail = engine.store.detail(&next.run_id).unwrap();
                assert!(
                    !detail.run.state.terminal(),
                    "must keep reading after a parent result while SendMessage is pending"
                );
                if let Some(a) = detail.approvals.into_iter().find(|a| a.state == "pending") {
                    assert_eq!(detail.run.state, RunState::WaitingUser);
                    break a;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("approval must arrive without a new prompt");
        engine
            .respond(&approval.id, json!({"decision":"accept"}))
            .await
            .unwrap();
        let done = finished(&engine, &next, false).await;
        assert_eq!(done.run.state, RunState::Completed);
        let children = engine
            .store
            .work_runs(&next.work_id)
            .unwrap()
            .into_iter()
            .filter(|r| r.agent_kind == "subagent")
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child);
        assert_eq!(children[0].title, "검토 전문가");
        let messages = engine.store.detail(&child).unwrap().messages;
        assert!(
            messages
                .iter()
                .any(|m| m.text == format!("followup-{turn}"))
        );
        assert!(
            messages
                .iter()
                .any(|m| m.text == format!("continued expert answer {turn}"))
        );
        previous = done.run.id;
    }
    engine.stop().await;
}

#[test]
fn claude_historical_message_nodes_reconcile_without_deleting_any_history() {
    let (engine, _dir) = setup();
    engine
        .store
        .upsert_host(Host {
            id: "local".into(),
            name: "fixture".into(),
            platform: "fixture".into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![Provider::Claude],
            error: None,
        })
        .unwrap();
    let receipt = engine
        .store
        .submit(request("ROOT", Provider::Claude))
        .unwrap();
    let root = engine.store.run(&receipt.run_id).unwrap();
    let session = "00000000-0000-4000-8000-000000000001";
    engine.store.claim_next("local").unwrap();
    engine.store.runtime_started(&root.id, session).unwrap();
    engine.store.delivered(&root.id, session, "turn").unwrap();
    let transcript = _dir.path().join(format!("{session}.jsonl"));
    std::fs::write(&transcript,json!({"type":"user","toolUseResult":{"agentId":"real-player"},"message":{"content":[{"type":"tool_result","tool_use_id":"spawn","content":"first answer"}]}}).to_string()+"\n").unwrap();
    engine
        .store
        .runtime_metadata(
            &root.id,
            None,
            None,
            Some(transcript.to_string_lossy().into()),
        )
        .unwrap();
    let update = |tool: &str, text: &str| SubagentUpdate {
        native_id: tool.into(),
        event_id: tool.into(),
        title: "player".into(),
        state: Some(RunState::Completed),
        prompt: Some("draw".into()),
        text: Some(text.into()),
        stats: None,
        started: true,
    };
    let first = engine
        .store
        .observe_subagent(&root.id, update("spawn", "first answer"))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = engine
        .store
        .observe_subagent(&root.id, update("message", "second answer"))
        .unwrap();
    engine
        .store
        .set_message(Message {
            phase: None,
            id: "late-original-output".into(),
            run_id: first.id.clone(),
            role: "assistant".into(),
            text: "output still delivered under the initial tool ID".into(),
            created_at: now() + 100,
        })
        .unwrap();
    engine
        .store
        .set_message(Message {
            phase: None,
            id: format!("{}:tool:message", root.id),
            run_id: root.id.clone(),
            role: "tool".into(),
            text: "SendMessage\n{\"to\":\"real-player\",\"message\":\"draw again\"}".into(),
            created_at: now(),
        })
        .unwrap();
    let before = engine.store.snapshot().unwrap();
    providers::claude::reconcile_history(&engine.store).unwrap();
    let latest = engine
        .store
        .subagent_by_native(&root.id, "real-player")
        .unwrap()
        .unwrap();
    assert_eq!(latest.id, second.id);
    assert_eq!(latest.session_id, first.id);
    assert_eq!(latest.continued_from, Some(first.id.clone()));
    let conversation = engine.store.detail(&latest.id).unwrap().conversation;
    assert!(
        conversation
            .iter()
            .any(|m| m.text.starts_with("first answer"))
    );
    assert!(conversation.iter().any(|m| m.text == "second answer"));
    assert_eq!(conversation.last().unwrap().id, "late-original-output");
    assert!(
        conversation
            .windows(2)
            .all(|pair| pair[0].created_at <= pair[1].created_at)
    );
    let children = engine.store.detail(&root.id).unwrap().children;
    assert_eq!(children.len(), 1);
    assert!(
        children[0]
            .messages
            .iter()
            .any(|m| m.text == "second answer")
    );
    let after = engine.store.snapshot().unwrap();
    assert_eq!(before.runs.len(), after.runs.len());
    assert_eq!(before.transmissions.len(), after.transmissions.len());
    providers::claude::reconcile_history(&engine.store).unwrap();
    assert_eq!(engine.store.snapshot().unwrap().last_seq, after.last_seq);
}

#[cfg(unix)]
#[tokio::test]
async fn codex_preserves_public_progress_and_final_message_phases() {
    let (mut engine, _dir) = setup();
    engine.config.codex_command = format!("{}/tests/fixtures/codex.py", env!("CARGO_MANIFEST_DIR"));
    engine.start().await.unwrap();
    let receipt = engine
        .store
        .submit(request("PHASE_FIXTURE", Provider::Codex))
        .unwrap();
    let detail = finished(&engine, &receipt, false).await;
    assert_eq!(
        detail.run.state,
        RunState::Completed,
        "{:?}",
        detail.run.error
    );
    let answers = detail
        .messages
        .iter()
        .filter(|m| m.role == "assistant")
        .collect::<Vec<_>>();
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0].phase.as_deref(), Some("commentary"));
    assert_eq!(answers[0].text, "visible progress");
    assert_eq!(answers[1].phase.as_deref(), Some("final_answer"));
    assert_eq!(
        serde_json::from_str::<Value>(&answers[1].text).unwrap()["turns"],
        1
    );
    let entry = engine
        .store
        .inbox(Some(&detail.run.conversation_id))
        .unwrap();
    assert_eq!(
        entry
            .iter()
            .find(|e| e.from_run_id == receipt.run_id)
            .unwrap()
            .result,
        answers[1].text
    );
    let events = engine.store.events(0, 1000).unwrap();
    assert!(
        events.iter().any(|e| e.kind == "message"
            && e.data["phase"] == "commentary"
            && e.data["text"] == "")
    );
    engine.stop().await;
}

#[tokio::test]
async fn interrupt_after_completion_returns_the_completed_run_without_another_event() {
    let (engine, _dir) = setup();
    engine
        .store
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
    let receipt = engine
        .store
        .submit(request("FINISHED", Provider::Mock))
        .unwrap();
    let run = engine.store.claim_next("local").unwrap().unwrap();
    engine.store.delivered(&run.id, "session", "turn").unwrap();
    engine
        .store
        .complete(&run.id, "done", UsageStats::default())
        .unwrap();
    let seq = engine.store.snapshot().unwrap().last_seq;
    for _ in 0..2 {
        let result = engine.interrupt(&receipt.run_id).await.unwrap();
        assert_eq!(result.state, RunState::Completed);
    }
    assert_eq!(engine.store.snapshot().unwrap().last_seq, seq);
    let second = engine
        .store
        .submit(request("DETACHED", Provider::Mock))
        .unwrap();
    engine.store.claim_next("local").unwrap();
    assert!(engine.interrupt(&second.run_id).await.is_err());
    assert_eq!(
        engine.store.run(&second.run_id).unwrap().state,
        RunState::Running
    );
}

#[tokio::test]
async fn approval_modes_apply_to_live_turns_and_restored_native_sessions() {
    for provider in [
        Some(Provider::Claude),
        cfg!(unix).then_some(Provider::Codex),
    ]
    .into_iter()
    .flatten()
    {
        let (mut engine, _dir) = setup();
        engine.config.codex_command =
            format!("{}/tests/fixtures/codex.py", env!("CARGO_MANIFEST_DIR"));
        claude_fixture(&mut engine);
        engine.start().await.unwrap();
        let first = engine
            .store
            .submit(request("APPROVAL_FIRST", provider.clone()))
            .unwrap();
        let first = finished(&engine, &first, true).await;
        assert_eq!(
            first.run.state,
            RunState::Completed,
            "{:?}",
            first.run.error
        );
        let answer = |d: &RunDetail| -> Value {
            serde_json::from_str(
                &d.messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "assistant")
                    .unwrap()
                    .text,
            )
            .unwrap()
        };
        let first_pid = answer(&first)["pid"].clone();
        let mut prior = first;
        for mode in [ApprovalMode::FullAccess, ApprovalMode::OnRequest] {
            let mut next = follow(&engine, &prior.run.id, &format!("APPROVAL_{mode:?}"));
            next.approval_mode = Some(mode);
            let next = engine.store.submit(next).unwrap();
            let next = finished(&engine, &next, false).await;
            assert_eq!(next.run.state, RunState::Completed, "{:?}", next.run.error);
            assert_eq!(next.run.session_key, prior.run.session_key);
            assert_eq!(next.run.session_id, prior.run.session_id);
            let value = answer(&next);
            assert_eq!(value["pid"], first_pid);
            if provider == Provider::Codex {
                assert_eq!(
                    value["approval_policy"],
                    if mode == ApprovalMode::FullAccess {
                        "never"
                    } else {
                        "on-request"
                    }
                );
                assert_eq!(
                    value["sandbox"]["type"],
                    if mode == ApprovalMode::FullAccess {
                        "dangerFullAccess"
                    } else {
                        "workspaceWrite"
                    }
                );
            } else {
                assert_eq!(
                    value["permission_mode"],
                    if mode == ApprovalMode::FullAccess {
                        "bypassPermissions"
                    } else {
                        "default"
                    }
                );
            }
            prior = next;
        }
        if provider == Provider::Claude {
            let mut next = follow(&engine, &prior.run.id, "APPROVAL_EDITS");
            next.approval_mode = Some(ApprovalMode::AcceptEdits);
            let next = engine.store.submit(next).unwrap();
            prior = finished(&engine, &next, false).await;
            assert_eq!(answer(&prior)["permission_mode"], "acceptEdits");
            assert_eq!(answer(&prior)["pid"], first_pid);
        }
        engine.stop().await;
        let engine = Engine::new(engine.store.clone(), engine.config.clone());
        engine.start().await.unwrap();
        let mut next = follow(&engine, &prior.run.id, "APPROVAL_RESTART");
        next.approval_mode = Some(ApprovalMode::FullAccess);
        let next = engine.store.submit(next).unwrap();
        let restored = finished(&engine, &next, false).await;
        assert_eq!(
            restored.run.state,
            RunState::Completed,
            "{:?}",
            restored.run.error
        );
        assert_eq!(restored.run.session_key, prior.run.session_key);
        let value = answer(&restored);
        assert_eq!(value["resumed"], true);
        assert_ne!(value["pid"], first_pid);
        if provider == Provider::Codex {
            assert_eq!(value["initial_policy"], "never");
            assert_eq!(value["initial_sandbox"], "danger-full-access");
        } else {
            assert_eq!(value["permission_mode"], "bypassPermissions");
        }
        engine.stop().await;
    }
}

#[tokio::test]
async fn rejected_claude_approval_change_never_sends_the_next_question() {
    let (mut engine, dir) = setup();
    claude_fixture(&mut engine);
    engine.start().await.unwrap();
    let first = engine
        .store
        .submit(request("APPROVAL_FIRST", Provider::Claude))
        .unwrap();
    let first = finished(&engine, &first, true).await;
    std::fs::write(dir.path().join("reject-permission"), "fixture").unwrap();
    let mut next = follow(&engine, &first.run.id, "MUST_NOT_REACH_MODEL");
    next.approval_mode = Some(ApprovalMode::FullAccess);
    let next = engine.store.submit(next).unwrap();
    let failed = finished(&engine, &next, false).await;
    assert_eq!(failed.run.state, RunState::Failed, "{:?}", failed.run.error);
    assert!(
        failed
            .run
            .error
            .as_deref()
            .unwrap()
            .contains("승인 모드 변경 실패")
    );
    let history: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join(format!(
            "{}.fixture.json",
            first.run.session_key.as_ref().unwrap()
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(failed.run.session_key, first.run.session_key);
    std::fs::remove_file(dir.path().join("reject-permission")).unwrap();
    let mut retry = follow(&engine, &failed.run.id, "CORRECTED_MODE");
    retry.approval_mode = Some(ApprovalMode::OnRequest);
    let retry = engine.store.submit(retry).unwrap();
    let retried = finished(&engine, &retry, false).await;
    assert_eq!(
        retried.run.state,
        RunState::Completed,
        "{:?}",
        retried.run.error
    );
    assert_eq!(retried.run.session_key, first.run.session_key);
    engine.stop().await;
}

fn claude_transport_fixture(engine: &mut Engine, dir: &tempfile::TempDir, mode: &str) {
    engine.config.claude_command = "node".into();
    engine.config.claude_args = vec![
        format!(
            "{}/tests/fixtures/claude-transport.mjs",
            env!("CARGO_MANIFEST_DIR")
        ),
        mode.into(),
        dir.path()
            .join("requests.jsonl")
            .to_string_lossy()
            .into_owned(),
    ];
}

#[tokio::test]
async fn claude_startup_failure_explains_exit_without_sending_question_or_exposing_stderr() {
    for (mode, hint) in [
        ("old-version", "CLI 버전"),
        ("git-bash", "Git Bash"),
        ("unknown", "오류 출력을"),
    ] {
        let (mut engine, dir) = setup();
        claude_transport_fixture(&mut engine, &dir, mode);
        engine.start().await.unwrap();
        let submitted = engine
            .store
            .submit(request("MUST_NOT_SEND", Provider::Claude))
            .unwrap();
        let failed = finished(&engine, &submitted, false).await;
        assert_eq!(failed.run.state, RunState::Failed, "{:?}", failed.run.error);
        let error = failed.run.error.unwrap();
        assert!(
            error.contains("종료 코드: 2") && error.contains(hint),
            "{error}"
        );
        assert!(!error.contains("must-never-appear") && !error.contains("private error details"));
        assert!(failed.run.turn_id.is_none());
        let sent = std::fs::read_to_string(dir.path().join("requests.jsonl")).unwrap();
        assert!(!sent.contains("MUST_NOT_SEND") && !sent.contains("\"type\":\"user\""));
        engine.stop().await;
    }
}

#[tokio::test]
async fn claude_closed_output_does_not_claim_that_a_live_process_has_exited() {
    let (mut engine, dir) = setup();
    let executable = dir
        .path()
        .join(format!("closed-output{}", std::env::consts::EXE_SUFFIX));
    let output =
        std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg("--edition=2024")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/closed-output.rs"
            ))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    engine.config.claude_command = executable.to_string_lossy().into_owned();
    engine.config.claude_args = vec![
        dir.path()
            .join("requests.jsonl")
            .to_string_lossy()
            .into_owned(),
    ];
    engine.start().await.unwrap();
    let submitted = engine
        .store
        .submit(request("MUST_NOT_SEND", Provider::Claude))
        .unwrap();
    let failed = finished(&engine, &submitted, false).await;
    let error = failed.run.error.unwrap();
    assert!(
        error.contains("제어 연결이 끊겼습니다") && error.contains("종료는 확인되지"),
        "{error}"
    );
    assert!(!error.contains("프로세스가 종료되었습니다"));
    engine.stop().await;
}

#[tokio::test]
async fn claude_stderr_larger_than_pipe_capacity_does_not_block_first_answer() {
    let (mut engine, dir) = setup();
    claude_transport_fixture(&mut engine, &dir, "stderr-flood");
    engine.start().await.unwrap();
    let submitted = engine
        .store
        .submit(request("FIXTURE_QUESTION", Provider::Claude))
        .unwrap();
    let done = finished(&engine, &submitted, false).await;
    assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
    assert!(
        done.messages
            .iter()
            .any(|m| m.role == "assistant" && m.text == "fixture answer")
    );
    engine.stop().await;
}

#[tokio::test]
async fn codex_native_commands_preserve_session_permissions_and_never_send_slash_as_prompt() {
    let (mut engine, dir) = setup();
    let record = dir.path().join("native-commands.jsonl");
    engine.config.codex_command = "node".into();
    engine.config.codex_args = vec![
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codex-commands.mjs"
        )
        .into(),
        record.to_string_lossy().into(),
    ];
    engine.start().await.unwrap();
    let mut initial = request("commands-initial", Provider::Codex);
    initial.model = "fixture-model".into();
    let receipt = engine.store.submit(initial).unwrap();
    finished(&engine, &receipt, false).await;
    let mut previous = receipt.run_id;
    for (index, text) in [
        "/plan inspect the code",
        "ordinary follow-up",
        "/review --branch feature/topic",
        "/skill:native-skill inspect files",
        "/compact",
    ]
    .iter()
    .enumerate()
    {
        let mut next = follow(&engine, &previous, &format!("command-{index}"));
        next.question = (*text).into();
        if text.starts_with("/review") {
            next.approval_mode = Some(ApprovalMode::FullAccess);
        }
        let receipt = engine.store.submit(next.clone()).unwrap();
        let duplicate = engine.store.submit(next).unwrap();
        assert_eq!(receipt.run_id, duplicate.run_id);
        finished(&engine, &receipt, false).await;
        let run = engine.store.run(&receipt.run_id).unwrap();
        assert_eq!(run.state, RunState::Completed, "{:?}", run.error);
        assert_eq!(run.session_key.as_deref(), Some("native-command-thread"));
        previous = run.id;
    }
    let lines: Vec<Value> = std::fs::read_to_string(record)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines
            .iter()
            .filter(|v| v["method"] == "thread/start")
            .count(),
        1
    );
    let review = lines
        .iter()
        .position(|v| v["method"] == "review/start")
        .unwrap();
    let resume = lines[..review]
        .iter()
        .rfind(|v| v["method"] == "thread/resume")
        .unwrap();
    assert_eq!(resume["params"]["sandbox"], "danger-full-access");
    assert_eq!(resume["params"]["approvalPolicy"], "never");
    assert_eq!(lines[review]["params"]["target"]["branch"], "feature/topic");
    let turns: Vec<_> = lines
        .iter()
        .filter(|v| v["method"] == "turn/start")
        .collect();
    assert_eq!(turns[1]["params"]["collaborationMode"]["mode"], "plan");
    assert_eq!(turns[2]["params"]["collaborationMode"]["mode"], "default");
    assert!(
        turns
            .iter()
            .any(|v| v["params"]["input"][0]["type"] == "skill")
    );
    for turn in turns {
        for item in turn["params"]["input"].as_array().unwrap() {
            assert!(!item["text"].as_str().unwrap_or("").starts_with('/'));
        }
    }
    assert_eq!(
        lines
            .iter()
            .filter(|v| v["method"] == "thread/compact/start")
            .count(),
        1
    );
    let mut invalid = follow(&engine, &previous, "unsupported-native");
    invalid.question = "/not-supported".into();
    assert!(engine.store.submit(invalid).is_err());
    engine.stop().await;
}

#[tokio::test]
async fn project_extension_changes_preserve_active_work_and_resume_the_same_session_next_turn() {
    use bibi_server::providers::extensions::{self, Edit};
    let (engine, _dir) = setup();
    let provider: ProviderConfig = serde_json::from_value(json!({
        "id":"extension-fixture", "name":"fixture", "host_id":"local", "adapter":"claude",
        "command":"node", "args":[concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/claude.mjs")]
    }))
    .unwrap();
    engine.store.save_provider(provider, None).unwrap();
    engine.start().await.unwrap();
    let mut submission = request("EXTENSION_FIRST", Provider::Claude);
    submission.provider_id = Some("extension-fixture".into());
    let first = engine.store.submit(submission).unwrap();
    let pending = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let detail = engine.store.detail(&first.run_id).unwrap();
            if detail.approvals.iter().any(|a| a.state == "pending") {
                break detail;
            }
            assert!(!detail.run.state.terminal(), "{:?}", detail.run.error);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let view = extensions::list(&engine, "p", "extension-fixture")
        .await
        .unwrap();
    extensions::update(
        &engine,
        "p",
        "extension-fixture",
        Edit {
            kind: "skill".into(),
            action: "add".into(),
            id: "fixture-only".into(),
            revision: view["revision"].as_str().unwrap().into(),
            value: json!("---\nname: fixture-only\ndescription: test\n---\nFixture only"),
        },
    )
    .await
    .unwrap();
    let after = engine.store.detail(&first.run_id).unwrap();
    assert_eq!(after.run.state, pending.run.state);
    assert_eq!(after.approvals[0].state, "pending");
    let a = finished(&engine, &first, true).await;
    assert_eq!(a.run.state, RunState::Completed, "{:?}", a.run.error);
    let a_result: Value = serde_json::from_str(
        &a.messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    let mut next = follow(&engine, &first.run_id, "EXTENSION_SECOND");
    next.provider_id = Some("extension-fixture".into());
    let receipt = engine.store.submit(next).unwrap();
    let b = finished(&engine, &receipt, false).await;
    assert_eq!(b.run.state, RunState::Completed, "{:?}", b.run.error);
    let b_result: Value = serde_json::from_str(
        &b.messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(a.run.session_key, b.run.session_key);
    assert_eq!(a.run.session_id, b.run.session_id);
    assert_eq!(b_result["resumed"], true);
    assert_eq!(b_result["turns"], 2);
    assert_ne!(a_result["pid"], b_result["pid"]);
    assert_eq!(a_result["permission_mode"], b_result["permission_mode"]);
    engine.stop().await;
}

#[tokio::test]
async fn claude_can_be_cancelled_during_initialization_without_sending_the_question() {
    let (mut engine, dir) = setup();
    claude_transport_fixture(&mut engine, &dir, "stall-start");
    engine.start().await.unwrap();
    let receipt = engine
        .store
        .submit(request("never-send-this-question", Provider::Claude))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_to_string(dir.path().join("requests.jsonl"))
                .is_ok_and(|text| text.contains("initialize"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    engine.interrupt(&receipt.run_id).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), finished(&engine, &receipt, false))
        .await
        .unwrap();
    assert_eq!(result.run.state, RunState::Interrupted);
    assert!(result.run.turn_id.is_none());
    engine.stop().await;
}

#[tokio::test]
async fn codex_startup_and_unacknowledged_stop_are_bounded() {
    for mode in ["stall-start", "ignore-stop"] {
        let (mut engine, dir) = setup();
        engine.config.codex_command = "node".into();
        engine.config.codex_args = vec![
            format!(
                "{}/tests/fixtures/codex-stop.mjs",
                env!("CARGO_MANIFEST_DIR")
            ),
            mode.into(),
            dir.path().join("requests.jsonl").to_string_lossy().into(),
        ];
        engine.start().await.unwrap();
        let receipt = engine.store.submit(request(mode, Provider::Codex)).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let ready = if mode == "stall-start" {
                    std::fs::read_to_string(dir.path().join("requests.jsonl"))
                        .is_ok_and(|s| s.contains("initialize"))
                } else {
                    engine.store.run(&receipt.run_id).unwrap().turn_id.is_some()
                };
                if ready {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        engine.interrupt(&receipt.run_id).await.unwrap();
        let detail =
            tokio::time::timeout(Duration::from_secs(11), finished(&engine, &receipt, false))
                .await
                .unwrap();
        assert_eq!(
            detail.run.state,
            RunState::Interrupted,
            "{:?}",
            detail.run.error
        );
        if mode == "stall-start" {
            let text = std::fs::read_to_string(dir.path().join("requests.jsonl")).unwrap();
            assert!(!text.contains("turn/start"));
        }
        engine.stop().await;
    }
}
