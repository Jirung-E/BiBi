use bibi_core::*;
use serde_json::json;
fn setup() -> Store {
    let s = Store::memory().unwrap();
    s.add_project(Project {
        id: "p".into(),
        name: "queue".into(),
        workspace: "/workspace".into(),
        guild_path: None,
        constraints: vec![],
    })
    .unwrap();
    s.upsert_host(Host {
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
    s
}
fn request(id: &str) -> Submission {
    serde_json::from_value(json!({"submission_id":id,"project_key":"p","question":id,"provider":"claude","host_id":"local","role":"coordinator","mode":"fresh","read_only":false,"approval_mode":"accept_edits"})).unwrap()
}
fn follow(r: &Run, id: &str) -> Submission {
    let mut q = request(id);
    q.mode = SubmitMode::Continue;
    q.target_run_id = Some(r.id.clone());
    q.work_id = Some(r.work_id.clone());
    q.expected_turn_id = r.turn_id.clone();
    q.expected_context_revision = Some(r.context_revision);
    q
}
fn active(s: &Store) -> Run {
    let r = s.claim_next("local").unwrap().unwrap();
    s.delivered(&r.id, "same-native-id", &format!("turn-{}", r.id))
        .unwrap()
}
fn done(s: &Store, r: &Run) {
    s.complete(&r.id, "answer", UsageStats::default()).unwrap();
}
fn dispatch(s: &Store, id: &str) -> Receipt {
    let q = s.prepare_queued_message(id).unwrap().unwrap();
    let receipt = s.submit(q).unwrap();
    s.finish_queued_message(id, Ok(receipt.clone())).unwrap();
    receipt
}
#[test]
fn queue_waits_for_completion_keeps_session_and_submission_order() {
    let s = setup();
    s.submit(request("first")).unwrap();
    let first = active(&s);
    let mut second = follow(&first, "second");
    second.approval_mode = None;
    let receipt = s.enqueue_message(second.clone()).unwrap();
    assert_eq!(receipt.status, "queued");
    assert_eq!(s.enqueue_message(second).unwrap().run_id, receipt.run_id);
    s.enqueue_message(follow(&first, "third")).unwrap();
    assert!(s.prepare_queued_message("second").unwrap().is_none());
    assert!(s.prepare_queued_message("third").unwrap().is_none());
    assert!(s.submit(follow(&first, "overtake")).is_err());
    done(&s, &first);
    let receipt = dispatch(&s, "second");
    let second = active(&s);
    assert_eq!(receipt.run_id, second.id);
    assert_eq!(second.session_id, first.session_id);
    assert_eq!(second.approval_mode, ApprovalMode::AcceptEdits);
    assert_eq!(second.continued_from.as_deref(), Some(first.id.as_str()));
    assert!(s.prepare_queued_message("third").unwrap().is_none());
    done(&s, &second);
    dispatch(&s, "third");
    let third = active(&s);
    assert_eq!(third.session_id, first.session_id);
    assert_eq!(third.continued_from.as_deref(), Some(second.id.as_str()));
    assert!(s.detail(&third.id).unwrap().queued_messages.is_empty());
}
#[test]
fn failed_turn_pauses_until_explicit_resume_and_cancel_unblocks_head() {
    let s = setup();
    s.submit(request("first")).unwrap();
    let first = active(&s);
    s.enqueue_message(follow(&first, "second")).unwrap();
    s.enqueue_message(follow(&first, "third")).unwrap();
    s.fail(&first.id, "stopped", true).unwrap();
    assert!(s.prepare_queued_message("second").unwrap().is_none());
    assert_eq!(
        s.detail(&first.id).unwrap().queued_messages[0].state,
        "blocked"
    );
    assert!(s.prepare_queued_message("third").unwrap().is_none());
    s.change_queued_message("second", true).unwrap();
    s.change_queued_message("third", false).unwrap();
    dispatch(&s, "third");
    assert_eq!(active(&s).session_id, first.session_id);
}
#[test]
fn queue_recovery_is_idempotent_after_dispatch_before_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("db");
    let s = setup(); // use persisted export through the same public setup below
    let project = s.project("p").unwrap();
    let host = s.snapshot().unwrap().hosts[0].clone();
    drop(s);
    let s = Store::open(&p).unwrap();
    s.add_project(project).unwrap();
    s.upsert_host(host).unwrap();
    s.submit(request("first")).unwrap();
    let first = active(&s);
    done(&s, &first);
    s.enqueue_message(follow(&first, "second")).unwrap();
    let prepared = s.prepare_queued_message("second").unwrap().unwrap();
    let receipt = s.submit(prepared.clone()).unwrap();
    assert!(s.change_queued_message("second", true).is_err());
    drop(s);
    let s = Store::open(&p).unwrap();
    let recovered = s.prepare_queued_message("second").unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(prepared).unwrap(),
        serde_json::to_value(&recovered).unwrap()
    );
    assert_eq!(s.submit(recovered).unwrap().run_id, receipt.run_id);
    s.finish_queued_message("second", Ok(receipt.clone()))
        .unwrap();
    assert_eq!(s.receipt("second").unwrap().unwrap().run_id, receipt.run_id);
    assert_eq!(s.snapshot().unwrap().runs.len(), 2);
}
#[test]
fn queue_validates_identity_and_conflicting_retry() {
    let s = setup();
    s.submit(request("first")).unwrap();
    let first = active(&s);
    let q = follow(&first, "second");
    s.enqueue_message(q.clone()).unwrap();
    let mut changed = q;
    changed.question = "different".into();
    assert!(s.enqueue_message(changed).is_err());
    let mut cross = follow(&first, "cross");
    cross.host_id = "remote".into();
    assert!(s.enqueue_message(cross).is_err());
    assert!(s.enqueue_message(request("fresh")).is_err());
}

#[test]
fn terminal_release_and_queued_submissions_cannot_race() {
    let s = setup();
    s.submit(request("first")).unwrap();
    let first = active(&s);
    done(&s, &first);
    let first = s.run(&first.id).unwrap();
    s.enqueue_message(follow(&first, "second")).unwrap();
    assert!(s.begin_claude_release(&first.id, "same-native-id").is_err());
    s.change_queued_message("second", true).unwrap();
    s.begin_claude_release(&first.id, "same-native-id").unwrap();
    assert!(s.enqueue_message(follow(&first, "third")).is_err());
    assert!(s.submit(follow(&first, "third")).is_err());
    s.finish_claude_release(&first.id, "/fixture/claude")
        .unwrap();
    assert_eq!(s.run(&first.id).unwrap().session_key, first.session_key);
    assert!(s.submit(follow(&first, "third")).is_ok());
}

#[test]
fn ordinary_submission_cannot_reuse_a_waiting_queue_id() {
    let s = setup();
    s.submit(request("first")).unwrap();
    let first = active(&s);
    done(&s, &first);
    let first = s.run(&first.id).unwrap();
    let queued = follow(&first, "waiting");
    s.enqueue_message(queued.clone()).unwrap();
    let mut changed = queued.clone();
    changed.question = "unexpected replacement".into();
    assert!(s.submit(changed).is_err());
    assert!(s.submit(queued).is_err());
    dispatch(&s, "waiting");
    assert_eq!(s.snapshot().unwrap().runs.len(), 2);
}
