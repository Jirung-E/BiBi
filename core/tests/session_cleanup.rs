use bibi_core::*;
use serde_json::json;

fn setup(s: &Store) -> Run {
    for id in ["p", "other"] {
        s.add_project(Project {
            id: id.into(),
            name: id.into(),
            workspace: "/fixture".into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    }
    s.upsert_host(Host {
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
    let request: Submission = serde_json::from_value(json!({"submission_id":"source","project_key":"p","question":"saved input","provider":"mock","model":"fixture","host_id":"local","role":"coordinator","mode":"fresh"})).unwrap();
    let receipt = s.submit(request).unwrap();
    s.claim_next("local").unwrap().unwrap();
    s.delivered(&receipt.run_id, "native", "turn").unwrap();
    s.complete(&receipt.run_id, "saved answer", UsageStats::default())
        .unwrap();
    s.run(&receipt.run_id).unwrap()
}
fn external(s: &Store, base: &Run, id: &str, state: RunState) -> Run {
    let mut run = base.clone();
    run.id = id.into();
    run.session_id = id.into();
    run.origin = Origin::External;
    run.state = state;
    run.session_key = Some(format!("native-{id}"));
    run.turn_id = Some(format!("turn-{id}"));
    s.import_external(run, vec![]).unwrap()
}
#[test]
fn cleanup_is_project_scoped_reversible_persistent_and_keeps_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let s = Store::open(&path).unwrap();
    let base = setup(&s);
    let first = external(&s, &base, "first", RunState::Completed);
    let mut second = first.clone();
    second.id = "second".into();
    second.continued_from = Some(first.id.clone());
    second.state = RunState::Disconnected;
    second.created_at += 1;
    s.import_external(
        second.clone(),
        vec![Message {
            attachments: vec![],
            id: "saved".into(),
            run_id: second.id.clone(),
            role: "assistant".into(),
            text: "keep this".into(),
            phase: None,
            created_at: now(),
        }],
    )
    .unwrap();
    let mut child = external(&s, &base, "child", RunState::Disconnected);
    child.host_id = "remote-host".into();
    child.agent_kind = "subagent".into();
    child.parent_session_id = Some(base.session_id().into());
    s.import_external(child.clone(), vec![]).unwrap();
    let mut other = external(&s, &base, "other-project", RunState::Disconnected);
    other.project_key = "other".into();
    s.import_external(other.clone(), vec![]).unwrap();
    let before = s.snapshot().unwrap();
    let result = s
        .cleanup_disconnected_sessions(
            "p",
            &[
                second.id.clone(),
                second.id.clone(),
                child.id.clone(),
                other.id.clone(),
            ],
        )
        .unwrap();
    assert_eq!(
        result.hidden_session_ids,
        vec![first.id.clone(), child.id.clone()]
    );
    assert_eq!(result.skipped_run_ids, vec![other.id.clone()]);
    let after = s.snapshot().unwrap();
    assert_eq!(after.removed_sessions.len(), 3);
    assert_eq!(after.runs.len() + 3, before.runs.len());
    assert_eq!(after.works.len(), before.works.len());
    assert_eq!(after.transmissions.len(), before.transmissions.len());
    assert_eq!(after.inbox.len(), before.inbox.len());
    assert_eq!(s.detail(&second.id).unwrap().messages[0].text, "keep this");
    assert_eq!(s.run(&child.id).unwrap().state, RunState::Disconnected);
    let count = s
        .events(0, 2000)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "session_visibility")
        .count();
    assert_eq!(count, 1);
    let retry = s
        .cleanup_disconnected_sessions("p", std::slice::from_ref(&second.id))
        .unwrap();
    assert!(retry.hidden_session_ids.is_empty());
    assert_eq!(retry.skipped_run_ids, vec![second.id.clone()]);
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.snapshot().unwrap().removed_sessions.len(), 3);
    // Rediscovery/reconnection must retain the user's hide preference and original native ID.
    second.state = RunState::Running;
    s.import_external(second.clone(), vec![]).unwrap();
    assert_eq!(s.snapshot().unwrap().removed_sessions.len(), 3);
    s.set_session_hidden(&first.id, false).unwrap();
    s.set_session_hidden(&child.id, false).unwrap();
    assert!(s.snapshot().unwrap().removed_sessions.is_empty());
    assert_eq!(s.run(&second.id).unwrap().session_key, first.session_key);
    assert_eq!(s.detail(&second.id).unwrap().messages[0].text, "keep this");
}
#[test]
fn cleanup_rechecks_latest_state_and_protects_all_active_turns_and_approvals() {
    let s = Store::memory().unwrap();
    let base = setup(&s);
    let mut selected = vec![];
    for (i, state) in [
        RunState::Queued,
        RunState::Running,
        RunState::WaitingUser,
        RunState::WaitingExpert,
        RunState::Uncertain,
        RunState::Completed,
        RunState::Failed,
        RunState::Interrupted,
    ]
    .into_iter()
    .enumerate()
    {
        let run = external(&s, &base, &format!("state-{i}"), state);
        selected.push(run.id);
    }
    let stale = external(&s, &base, "stale", RunState::Disconnected);
    selected.push(stale.id.clone());
    let mut next = stale.clone();
    next.id = "next".into();
    next.continued_from = Some(stale.id.clone());
    next.created_at += 1;
    s.import_external(next.clone(), vec![]).unwrap();
    let mut active = next.clone();
    active.id = "active-member".into();
    active.state = RunState::Running;
    active.created_at -= 1;
    s.import_external(active, vec![]).unwrap();
    selected.push(next.id);
    for status in ["pending", "sending", "uncertain"] {
        let run = external(&s, &base, status, RunState::Disconnected);
        s.add_approval(Approval {
            id: format!("approval-{status}"),
            run_id: run.id.clone(),
            native_id: json!(1),
            kind: "fixture".into(),
            title: "approval".into(),
            detail: json!({}),
            state: status.into(),
            created_at: now(),
        })
        .unwrap();
        selected.push(run.id);
    }
    let restored = external(&s, &base, "reconnected", RunState::Disconnected);
    s.observe(&restored.id, RunState::Running, "running", None)
        .unwrap();
    selected.push(restored.id);
    selected.push("missing".into());
    let allowed = external(&s, &base, "allowed", RunState::Disconnected);
    selected.push(allowed.id.clone());
    let result = s.cleanup_disconnected_sessions("p", &selected).unwrap();
    assert_eq!(result.hidden_session_ids, vec![allowed.id]);
    assert_eq!(result.skipped_run_ids.len(), selected.len() - 1);
    assert_eq!(s.snapshot().unwrap().removed_sessions.len(), 1);
    assert!(s.cleanup_disconnected_sessions("p", &[]).is_err());
    assert!(
        s.cleanup_disconnected_sessions("p", &vec!["allowed".into(); 2001])
            .is_err()
    );
    assert!(
        s.cleanup_disconnected_sessions("missing", &selected)
            .is_err()
    );
}
#[test]
fn pending_direct_input_is_not_hidden_when_the_connection_drops() {
    let s = Store::memory().unwrap();
    setup(&s);
    let mut request: Submission=serde_json::from_value(json!({"submission_id":"active","project_key":"p","question":"input","provider":"mock","model":"fixture","host_id":"local","role":"coordinator","mode":"fresh"})).unwrap();
    let receipt = s.submit(request.clone()).unwrap();
    s.claim_next("local").unwrap();
    let run = s.delivered(&receipt.run_id, "native2", "turn2").unwrap();
    request.submission_id = "pending-input".into();
    request.mode = SubmitMode::Steer;
    request.target_run_id = Some(run.id.clone());
    request.expected_turn_id = run.turn_id;
    request.expected_context_revision = Some(1);
    s.submit(request).unwrap();
    s.observe(&run.id, RunState::Disconnected, "disconnected", None)
        .unwrap();
    let result = s
        .cleanup_disconnected_sessions("p", std::slice::from_ref(&run.id))
        .unwrap();
    assert!(result.hidden_session_ids.is_empty());
    assert_eq!(result.skipped_run_ids, vec![run.id.clone()]);
    assert_eq!(s.inputs(&run.id).unwrap()[0].state, "accepted");
    assert_eq!(s.run(&run.id).unwrap().state, RunState::Disconnected);
}
