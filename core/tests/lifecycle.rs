use bibi_core::*;

fn setup(store: &Store) {
    store
        .add_project(Project {
            id: "p".into(),
            name: "BiBi".into(),
            workspace: "/workspace".into(),
            guild_path: None,
            constraints: vec!["Claude 실호출 금지".into()],
        })
        .unwrap();
    store
        .upsert_host(Host {
            id: "local".into(),
            name: "local".into(),
            platform: "test".into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![Provider::Mock, Provider::Codex, Provider::Ollama],
            error: None,
        })
        .unwrap();
}
fn request(key: &str) -> Submission {
    Submission {
        attachments: vec![],
        submission_id: key.into(),
        project_key: "p".into(),
        work_id: None,
        title: Some("독립 업무".into()),
        question: "문제를 조사해라".into(),
        provider: Provider::Mock,
        provider_id: None,
        model: "mock".into(),
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
fn finish(s: &Store, r: &Receipt, result: &str) {
    let run = s.claim_next("local").unwrap().unwrap();
    assert_eq!(run.id, r.run_id);
    s.delivered(&run.id, "native-session", "turn-1").unwrap();
    s.append_output(&run.id, &format!("out-{}", run.id), "assistant", result)
        .unwrap();
    s.complete(&run.id, result, UsageStats::default()).unwrap();
}
#[test]
fn acceptance_survives_reopen_and_retry_never_runs_twice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite3");
    let s = Store::open(&path).unwrap();
    setup(&s);
    let first = s.submit(request("same-key")).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    let retry = s.submit(request("same-key")).unwrap();
    assert_eq!(first.run_id, retry.run_id);
    assert_eq!(s.snapshot().unwrap().runs.len(), 1);
    let mut changed = request("same-key");
    changed.question = "다른 질문".into();
    assert!(matches!(s.submit(changed), Err(Error::Conflict(_))));
    assert_eq!(s.snapshot().unwrap().runs.len(), 1);
}
#[test]
fn simultaneous_connections_share_idempotency_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite3");
    let s = Store::open(&path).unwrap();
    setup(&s);
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let p = path.clone();
            std::thread::spawn(move || Store::open(p).unwrap().submit(request("race")).unwrap())
        })
        .collect();
    let ids: Vec<_> = handles
        .into_iter()
        .map(|t| t.join().unwrap().run_id)
        .collect();
    assert!(ids.iter().all(|id| id == &ids[0]));
    assert_eq!(s.snapshot().unwrap().runs.len(), 1);
}
#[test]
fn followup_uses_new_run_and_bounded_grounded_context() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("first")).unwrap();
    let long = "결과".repeat(4000);
    finish(&s, &a, &long);
    let mut follow = request("follow");
    follow.target_run_id = Some(a.run_id.clone());
    follow.expected_context_revision = Some(1);
    let b = s.submit(follow).unwrap();
    let run = s.run(&b.run_id).unwrap();
    assert_ne!(a.run_id, b.run_id);
    assert_eq!(a.work_id, b.work_id);
    assert_eq!(run.session_key, None);
    assert_eq!(run.context.source_runs, vec![a.run_id]);
    assert_eq!(run.context.constraints, vec!["Claude 실호출 금지"]);
    assert_eq!(
        run.context
            .previous_answer_excerpt
            .as_ref()
            .unwrap()
            .chars()
            .count(),
        6000
    );
    assert!(run.context.excerpt_truncated);
    assert!(
        run.context
            .references
            .iter()
            .any(|r| r.source.starts_with("bibi://inbox/"))
    );
    assert_eq!(s.detail(&b.run_id).unwrap().messages.len(), 1);
}
#[test]
fn independent_workspace_writers_and_readonly_experts_can_run_together() {
    let s = Store::memory().unwrap();
    setup(&s);
    s.submit(request("write-a")).unwrap();
    s.claim_next("local").unwrap().unwrap();
    let second = s.submit(request("write-b")).unwrap();
    assert_eq!(s.claim_next("local").unwrap().unwrap().id, second.run_id);
    let mut expert = request("reader");
    expert.read_only = true;
    expert.role = "DB".into();
    let e = s.submit(expert).unwrap();
    assert_eq!(s.claim_next("local").unwrap().unwrap().id, e.run_id);
}
#[test]
fn crash_marks_ambiguous_runs_and_does_not_replay_effects() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("a")).unwrap();
    s.claim_next("local").unwrap().unwrap();
    s.delivered(&a.run_id, "native", "turn").unwrap();
    assert_eq!(s.recover_host("local").unwrap(), 1);
    assert_eq!(s.run(&a.run_id).unwrap().state, RunState::Uncertain);
    let other = s.submit(request("b")).unwrap();
    assert_eq!(s.claim_next("local").unwrap().unwrap().id, other.run_id);
    assert_eq!(s.run(&a.run_id).unwrap().state, RunState::Uncertain);
    let mut follow = request("resume-after-crash");
    follow.mode = SubmitMode::Continue;
    follow.target_run_id = Some(a.run_id.clone());
    follow.expected_turn_id = Some("turn".into());
    follow.expected_context_revision = Some(1);
    assert!(matches!(s.submit(follow.clone()), Err(Error::Conflict(_))));
    s.resolve_uncertain(&a.run_id).unwrap();
    let receipt = s.submit(follow).unwrap();
    let resumed = s.claim_next("local").unwrap().unwrap();
    assert_eq!(resumed.id, receipt.run_id);
    assert_eq!(resumed.session_key.as_deref(), Some("native"));
}
#[test]
fn stale_target_steer_and_context_are_rejected_atomically() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("a")).unwrap();
    s.claim_next("local").unwrap().unwrap();
    s.delivered(&a.run_id, "native", "current").unwrap();
    let mut steer = request("steer");
    steer.mode = SubmitMode::Steer;
    steer.target_run_id = Some(a.run_id.clone());
    steer.expected_turn_id = Some("old".into());
    assert!(matches!(s.submit(steer.clone()), Err(Error::Conflict(_))));
    assert!(s.receipt("steer").unwrap().is_none());
    steer.expected_turn_id = Some("current".into());
    s.submit(steer).unwrap();
    assert_eq!(s.pending_inputs(&a.run_id).unwrap().len(), 1);
    let mut follow = request("follow");
    follow.work_id = Some(a.work_id);
    follow.expected_context_revision = Some(0);
    assert!(matches!(s.submit(follow), Err(Error::Conflict(_))));
    assert_eq!(s.snapshot().unwrap().runs.len(), 1);
}
#[test]
fn late_reply_is_durable_without_overwriting_newer_work() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("parent")).unwrap();
    finish(&s, &a, "기존 결론");
    let mut child = request("expert");
    child.target_run_id = Some(a.run_id.clone());
    child.expected_context_revision = Some(1);
    child.read_only = true;
    let b = s.submit(child).unwrap();
    s.claim_next("local").unwrap().unwrap();
    s.delivered(&b.run_id, "expert", "t").unwrap();
    let update = ContextUpdate {
        expected_revision: 1,
        goal: "새 목표".into(),
        constraints: vec!["Claude 실호출 금지".into()],
        ..Default::default()
    };
    s.update_context(&a.work_id, update).unwrap();
    let reply = s
        .complete(&b.run_id, "늦은 전문가 결과", UsageStats::default())
        .unwrap();
    let w = s.work(&a.work_id).unwrap();
    assert_eq!(w.goal, "새 목표");
    assert_eq!(w.context_revision, 2);
    assert_eq!(reply.context_revision, 1);
    assert_eq!(s.inbox(Some(&w.conversation_id)).unwrap().len(), 2);
    assert_eq!(
        s.snapshot()
            .unwrap()
            .transmissions
            .iter()
            .filter(|t| t.kind == "reply")
            .count(),
        1
    );
}
#[test]
fn replay_does_not_reset_send_time_and_reads_do_not_emit_events() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("a")).unwrap();
    s.claim_next("local").unwrap().unwrap();
    s.delivered(&a.run_id, "native", "t").unwrap();
    let original = s.snapshot().unwrap().transmissions[0].sent_at;
    s.delivered(&a.run_id, "native", "t").unwrap();
    let snapshot = s.snapshot().unwrap();
    let seq = snapshot.last_seq;
    assert_eq!(snapshot.transmissions.len(), 1);
    assert_eq!(snapshot.transmissions[0].sent_at, original);
    for _ in 0..5 {
        s.detail(&a.run_id).unwrap();
        s.snapshot().unwrap();
        s.events(0, 100).unwrap();
    }
    assert_eq!(s.snapshot().unwrap().last_seq, seq);
    let first = s.events(0, 3).unwrap();
    let rest = s.events(first.last().unwrap().seq, 100).unwrap();
    assert!(rest.iter().all(|e| e.seq > first.last().unwrap().seq));
    assert!((arrow_opacity(original, original + 30_000, 30.0, 0.15) - 0.575).abs() < 0.0001);
}
#[test]
fn mandatory_constraints_cannot_be_dropped_by_context_edit() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("a")).unwrap();
    let update = ContextUpdate {
        expected_revision: 1,
        goal: "새 목표".into(),
        ..Default::default()
    };
    assert!(matches!(
        s.update_context(&a.work_id, update),
        Err(Error::Invalid(_))
    ));
    assert_eq!(s.work(&a.work_id).unwrap().context_revision, 1);
}
#[test]
fn provider_absent_from_host_is_not_available() {
    let s = Store::memory().unwrap();
    setup(&s);
    let mut r = request("claude");
    r.provider = Provider::Claude;
    assert!(matches!(s.submit(r), Err(Error::Unsupported(_))));
    assert!(s.snapshot().unwrap().runs.is_empty());
}

#[test]
fn interrupted_late_result_is_retained_without_promoting_state() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("a")).unwrap();
    s.claim_next("local").unwrap().unwrap();
    s.delivered(&a.run_id, "native", "t").unwrap();
    s.fail(&a.run_id, "사용자 중단", true).unwrap();
    let entry = s
        .complete(&a.run_id, "중단 후 도착", UsageStats::default())
        .unwrap();
    assert!(entry.late);
    assert_eq!(s.run(&a.run_id).unwrap().state, RunState::Interrupted);
    assert_eq!(
        s.inbox_entry(&entry.response_id).unwrap().result,
        "중단 후 도착"
    );
}
#[test]
fn runtime_observation_does_not_refresh_work_report() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("a")).unwrap();
    s.claim_next("local").unwrap().unwrap();
    let report = Activity {
        revision: 1,
        phase: "조사".into(),
        summary: "근거 확인".into(),
        wait_reason: Some("전문가".into()),
        next_action: "종합".into(),
        references: vec![],
        reported_at: 0,
    };
    let r = s.report(&a.run_id, report.clone()).unwrap();
    s.observe(&a.run_id, RunState::Running, "진행 중", None)
        .unwrap();
    assert_eq!(
        s.run(&a.run_id).unwrap().activity.unwrap().reported_at,
        r.activity.unwrap().reported_at
    );
    assert!(matches!(
        s.report(&a.run_id, report),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn forwarded_jobs_compare_the_entire_frozen_context_and_preserve_terminal_state() {
    let central = Store::memory().unwrap();
    setup(&central);
    central
        .upsert_host(Host {
            id: "peer".into(),
            name: "peer".into(),
            platform: "test".into(),
            kind: "peer".into(),
            connected: true,
            observed_at: now(),
            providers: vec![Provider::Mock],
            error: None,
        })
        .unwrap();
    central
        .set_setting("host_workspace:peer:p", &"/remote")
        .unwrap();
    let mut request = request("remote");
    request.host_id = "peer".into();
    let receipt = central.submit(request).unwrap();
    let job = ForwardJob {
        run: central.claim_next("peer").unwrap().unwrap(),
        project: central.project("p").unwrap(),
        work: central.work(&receipt.work_id).unwrap(),
    };
    let remote = Store::memory().unwrap();
    remote.accept_forwarded(job.clone()).unwrap();
    remote.accept_forwarded(job.clone()).unwrap();
    let mut changed = job.clone();
    changed.run.context.constraints.clear();
    assert!(matches!(
        remote.accept_forwarded(changed),
        Err(Error::Conflict(_))
    ));
    let native = remote.claim_next("local").unwrap().unwrap();
    remote.delivered(&native.id, "session", "turn").unwrap();
    central.fail(&native.id, "cancelled", true).unwrap();
    remote
        .complete(&native.id, "late result", UsageStats::default())
        .unwrap();
    let snapshot = remote.snapshot().unwrap();
    central
        .mirror_remote(
            "peer",
            remote.detail(&native.id).unwrap(),
            snapshot.transmissions,
        )
        .unwrap();
    let detail = central.detail(&native.id).unwrap();
    assert_eq!(detail.run.state, RunState::Interrupted);
    assert!(detail.inbox[0].late);
    assert_eq!(
        detail.messages.iter().filter(|m| m.role == "user").count(),
        1
    );
}
#[test]
fn queued_cancel_cannot_cancel_a_run_already_claimed_by_the_dispatcher() {
    let s = Store::memory().unwrap();
    setup(&s);
    let receipt = s.submit(request("cancel")).unwrap();
    s.claim_next("local").unwrap().unwrap();
    assert!(s.cancel_queued(&receipt.run_id).unwrap().is_none());
    assert_eq!(s.run(&receipt.run_id).unwrap().state, RunState::Running);
}

#[test]
fn terminal_runtime_settles_pending_controls_without_faking_delivery() {
    let s = Store::memory().unwrap();
    setup(&s);
    let receipt = s.submit(request("control-close")).unwrap();
    s.claim_next("local").unwrap();
    s.delivered(&receipt.run_id, "native", "turn").unwrap();
    let mut steer = request("pending");
    steer.mode = SubmitMode::Steer;
    steer.target_run_id = Some(receipt.run_id.clone());
    steer.expected_turn_id = Some("turn".into());
    s.submit(steer).unwrap();
    s.complete(&receipt.run_id, "finished", UsageStats::default())
        .unwrap();
    assert_eq!(s.inputs(&receipt.run_id).unwrap()[0].state, "failed");
    assert!(
        !s.snapshot()
            .unwrap()
            .transmissions
            .iter()
            .any(|e| e.kind == "steer")
    );
}

#[test]
fn dispatch_queries_exclude_history_and_already_attached_remote_queue() {
    let s = Store::memory().unwrap();
    setup(&s);
    let first = s.submit(request("history")).unwrap();
    finish(&s, &first, "done");
    let next = s.submit(request("queue")).unwrap();
    assert_eq!(s.dispatch_runs().unwrap().len(), 1);
    assert!(
        s.claim_next_excluding("local", std::slice::from_ref(&next.run_id))
            .unwrap()
            .is_none()
    );
    assert_eq!(s.work_runs(&first.work_id).unwrap().len(), 1);
    assert_eq!(s.request_transmissions(&first.request_id).unwrap().len(), 1);
}

#[test]
fn continuation_keeps_session_and_full_history_without_mixing_experts() {
    let s = Store::memory().unwrap();
    setup(&s);
    let first = s.submit(request("session-first")).unwrap();
    finish(&s, &first, "첫 답변 전체");
    let original = s.run(&first.run_id).unwrap();
    let mut expert = request("expert");
    expert.target_run_id = Some(first.run_id.clone());
    expert.expected_context_revision = Some(1);
    expert.role = "전문가: DB".into();
    expert.read_only = true;
    let expert = s.submit(expert).unwrap();
    finish(&s, &expert, "전문가의 별도 대화");
    let mut follow = request("continue");
    follow.mode = SubmitMode::Continue;
    follow.target_run_id = Some(first.run_id.clone());
    follow.expected_context_revision = Some(1);
    follow.expected_turn_id = original.turn_id.clone();
    follow.question = "그다음은?".into();
    follow.model = "different-model".into();
    let second = s.submit(follow.clone()).unwrap();
    assert_eq!(s.submit(follow.clone()).unwrap().run_id, second.run_id);
    let next = s.run(&second.run_id).unwrap();
    assert_eq!(next.session_id, original.session_id);
    assert_eq!(next.model, "different-model");
    assert_eq!(s.run(&original.id).unwrap().model, original.model);
    assert_eq!(next.continued_from.as_deref(), Some(original.id.as_str()));
    let detail = s.detail(&next.id).unwrap();
    assert_eq!(detail.conversation.len(), 3);
    assert_eq!(detail.conversation[1].text, "첫 답변 전체");
    assert!(
        detail
            .conversation
            .iter()
            .all(|m| !m.text.contains("전문가의 별도"))
    );
    follow.submission_id = "duplicate-turn".into();
    assert!(matches!(s.submit(follow), Err(Error::Conflict(_))));
    assert!(
        s.claim_next_excluding("local", std::slice::from_ref(&original.id))
            .unwrap()
            .is_none()
    );
    finish(&s, &second, "둘째 답변");
    assert_eq!(s.detail(&second.run_id).unwrap().conversation.len(), 4);
    let mut fresh = request("fresh-branch");
    fresh.target_run_id = Some(second.run_id);
    fresh.expected_context_revision = Some(1);
    let branch = s.submit(fresh).unwrap();
    let branch = s.run(&branch.run_id).unwrap();
    assert_ne!(branch.session_id, original.session_id);
    assert_eq!(
        branch.parent_session_id.as_deref(),
        Some(original.session_id.as_str())
    );
    assert_eq!(s.detail(&branch.id).unwrap().conversation.len(), 1);
}

#[test]
fn continuation_rejects_active_external_and_changed_provider_or_permissions() {
    let s = Store::memory().unwrap();
    setup(&s);
    let first = s.submit(request("a")).unwrap();
    let mut follow = request("b");
    follow.mode = SubmitMode::Continue;
    follow.target_run_id = Some(first.run_id.clone());
    follow.expected_context_revision = Some(1);
    assert!(s.submit(follow.clone()).is_err());
    finish(&s, &first, "done");
    follow.expected_turn_id = Some("turn-1".into());
    follow.read_only = true;
    assert!(s.submit(follow.clone()).is_err());
    follow.read_only = false;
    follow.provider = Provider::Codex;
    assert!(s.submit(follow).is_err());
}

#[test]
fn messages_with_same_timestamp_keep_receipt_order_across_updates() {
    let s = Store::memory().unwrap();
    setup(&s);
    let receipt = s.submit(request("ordering")).unwrap();
    let timestamp = now() + 10000;
    for (id, text) in [
        ("z-answer", "answer first"),
        ("a-summary", "completion second"),
    ] {
        s.set_message(Message {
            attachments: vec![],
            phase: None,
            id: id.into(),
            run_id: receipt.run_id.clone(),
            role: "assistant".into(),
            text: text.into(),
            created_at: timestamp,
        })
        .unwrap();
    }
    let messages = s.detail(&receipt.run_id).unwrap().messages;
    assert_eq!(messages[1].text, "answer first");
    assert_eq!(messages[2].text, "completion second");
}

#[test]
fn public_message_phases_survive_streaming_updates_and_legacy_records() {
    let legacy: Message = serde_json::from_value(serde_json::json!({
        "id":"legacy", "run_id":"old", "role":"assistant", "text":"old answer", "created_at":1
    }))
    .unwrap();
    assert!(legacy.phase.is_none());
    assert_eq!(legacy.role, "assistant");
    let store = Store::memory().unwrap();
    setup(&store);
    let receipt = store.submit(request("phases")).unwrap();
    store.claim_next("local").unwrap();
    store
        .set_message(Message {
            attachments: vec![],
            id: "progress".into(),
            run_id: receipt.run_id.clone(),
            role: "assistant".into(),
            phase: Some("commentary".into()),
            text: String::new(),
            created_at: 1,
        })
        .unwrap();
    store
        .append_output(&receipt.run_id, "progress", "assistant", "visible progress")
        .unwrap();
    let mut streamed = store
        .detail(&receipt.run_id)
        .unwrap()
        .messages
        .into_iter()
        .find(|m| m.id == "progress")
        .unwrap();
    assert_eq!(streamed.phase.as_deref(), Some("commentary"));
    assert_eq!(streamed.text, "visible progress");
    // A completion payload without phase must not erase metadata received at item/start.
    streamed.phase = None;
    store.set_message(streamed).unwrap();
    let detail = store.detail(&receipt.run_id).unwrap();
    let progress = detail.messages.iter().find(|m| m.id == "progress").unwrap();
    assert_eq!(progress.phase.as_deref(), Some("commentary"));
    assert_eq!(progress.created_at, 1);
    assert_eq!(
        detail
            .conversation
            .iter()
            .find(|m| m.id == "progress")
            .unwrap()
            .phase,
        progress.phase
    );
}

#[test]
fn search_finds_older_turns_but_opens_latest_and_excludes_hidden_and_other_projects() {
    let s = Store::memory().unwrap();
    setup(&s);
    let first = s.submit(request("search-first")).unwrap();
    finish(&s, &first, "고유 검색어 100%_한글 답변");
    let previous = s.run(&first.run_id).unwrap();
    let mut next = request("search-follow");
    next.mode = SubmitMode::Continue;
    next.target_run_id = Some(previous.id.clone());
    next.expected_turn_id = previous.turn_id;
    next.expected_context_revision = Some(1);
    let second = s.submit(next).unwrap();
    finish(&s, &second, "후속 답변");
    let hits = s.search_sessions("p", "100%_한글").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].run.id, second.run_id);
    assert!(hits[0].excerpt.contains("100%_한글"));
    assert_eq!(s.search_sessions("p", "독립 업무").unwrap().len(), 1);
    s.add_project(Project {
        id: "other".into(),
        name: "다른 프로젝트".into(),
        workspace: "/other".into(),
        guild_path: None,
        constraints: vec![],
    })
    .unwrap();
    let mut other = request("other-search");
    other.project_key = "other".into();
    other.question = "100%_한글".into();
    s.submit(other).unwrap();
    assert_eq!(s.search_sessions("p", "100%_한글").unwrap().len(), 1);
    s.set_session_hidden(&second.run_id, true).unwrap();
    assert!(s.search_sessions("p", "100%_한글").unwrap().is_empty());
    assert!(s.search_sessions("p", &"한".repeat(161)).is_err());
    assert!(s.search_sessions("missing", "").is_err());
}

#[test]
fn queued_cancellation_and_stop_intent_never_revive_completion() {
    let s = Store::memory().unwrap();
    setup(&s);
    let first = s.submit(request("blocking")).unwrap();
    s.claim_next("local").unwrap();
    let queued = s.submit(request("waiting")).unwrap();
    assert_eq!(s.run(&queued.run_id).unwrap().state, RunState::Queued);
    s.cancel_queued(&queued.run_id).unwrap();
    assert_eq!(s.run(&queued.run_id).unwrap().state, RunState::Interrupted);
    s.delivered(&first.run_id, "native", "turn").unwrap();
    assert_eq!(s.run(&first.run_id).unwrap().phase, "응답 대기");
    s.append_output(&first.run_id, "answer", "assistant", "응답")
        .unwrap();
    assert_eq!(s.run(&first.run_id).unwrap().phase, "답변 작성 중");
    assert_eq!(
        s.interrupt_requested(&first.run_id).unwrap().phase,
        "중단 요청 중"
    );
    s.complete(&first.run_id, "응답", UsageStats::default())
        .unwrap();
    assert_eq!(
        s.interrupt_requested(&first.run_id).unwrap().state,
        RunState::Completed
    );
}

#[test]
fn cancelling_a_queued_followup_keeps_the_native_identity_for_resume() {
    let s = Store::memory().unwrap();
    setup(&s);
    let first = s.submit(request("first-to-resume")).unwrap();
    finish(&s, &first, "기존 답변");
    let previous = s.run(&first.run_id).unwrap();
    let mut follow = request("cancel-follow");
    follow.mode = SubmitMode::Continue;
    follow.target_run_id = Some(previous.id);
    follow.expected_turn_id = previous.turn_id;
    follow.expected_context_revision = Some(1);
    let cancelled = s.submit(follow.clone()).unwrap();
    s.cancel_queued(&cancelled.run_id).unwrap();
    let saved = s.run(&cancelled.run_id).unwrap();
    assert_eq!(saved.session_key.as_deref(), Some("native-session"));
    assert!(saved.turn_id.is_none());
    follow.submission_id = "resume-cancelled".into();
    follow.target_run_id = Some(saved.id);
    follow.expected_turn_id = None;
    let next = s.submit(follow).unwrap();
    let run = s.run(&next.run_id).unwrap();
    assert_eq!(run.session_key, saved.session_key);
    assert_eq!(run.session_id, saved.session_id);
}
