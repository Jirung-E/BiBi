use bibi_core::*;
use serde_json::json;

fn setup(s: &Store) {
    for id in ["p", "other"] {
        s.add_project(Project {
            id: id.into(),
            name: id.into(),
            workspace: format!("/{id}"),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    }
    s.upsert_host(Host {
        id: "local".into(),
        name: "local".into(),
        platform: "test".into(),
        kind: "local".into(),
        connected: true,
        observed_at: now(),
        providers: vec![Provider::Mock],
        error: None,
    })
    .unwrap();
}
fn request(id: &str) -> Submission {
    serde_json::from_value(json!({"submission_id":id,"project_key":"p","question":"그룹 검증","provider":"mock","model":"m","host_id":"local","role":"coordinator","mode":"fresh","read_only":true})).unwrap()
}
fn complete(s: &Store, receipt: &Receipt) -> Run {
    assert_eq!(s.claim_next("local").unwrap().unwrap().id, receipt.run_id);
    s.delivered(&receipt.run_id, "native-a", "turn-a").unwrap();
    s.append_output(
        &receipt.run_id,
        &format!("out:{}", receipt.run_id),
        "assistant",
        "원래 답변",
    )
    .unwrap();
    s.complete(&receipt.run_id, "원래 답변", UsageStats::default())
        .unwrap();
    s.run(&receipt.run_id).unwrap()
}
#[test]
fn memberships_survive_continuation_restart_hiding_and_removal_without_cloning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("groups.sqlite3");
    let s = Store::open(&path).unwrap();
    setup(&s);
    let first = s.submit(request("a")).unwrap();
    let a = complete(&s, &first);
    let second = s.submit(request("b")).unwrap();
    complete(&s, &second);
    let before = s.detail(&a.id).unwrap();
    let count = s.snapshot().unwrap().transmissions.len();
    let groups = s
        .set_session_groups(
            &a.id,
            vec![first.work_id.clone(), second.work_id.clone()],
            vec![first.work_id.clone()],
        )
        .unwrap();
    assert_eq!(s.snapshot().unwrap().runs.len(), 2);
    assert_eq!(s.snapshot().unwrap().transmissions.len(), count);
    assert_eq!(
        serde_json::to_value(s.detail(&a.id).unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let mut follow = request("continue");
    follow.mode = SubmitMode::Continue;
    follow.target_run_id = Some(a.id.clone());
    follow.expected_turn_id = a.turn_id.clone();
    follow.expected_context_revision = Some(s.work(&a.work_id).unwrap().context_revision);
    let next = s.submit(follow).unwrap();
    let continued = s.run(&next.run_id).unwrap();
    assert_eq!(continued.session_id(), a.session_id());
    assert_eq!(continued.work_id, a.work_id);
    assert_eq!(continued.continued_from.as_deref(), Some(a.id.as_str()));
    assert_eq!(s.snapshot().unwrap().session_groups, vec![groups.clone()]);
    s.cancel_queued(&next.run_id).unwrap();
    s.set_session_hidden(&next.run_id, true).unwrap();
    drop(s);
    let s = Store::open(path).unwrap();
    assert_eq!(s.snapshot().unwrap().session_groups, vec![groups.clone()]);
    s.set_session_hidden(&next.run_id, false).unwrap();
    let empty = s
        .set_session_groups(&next.run_id, vec![], groups.work_ids)
        .unwrap();
    assert!(empty.work_ids.is_empty());
    assert_eq!(s.snapshot().unwrap().runs.len(), 3);
    assert_eq!(s.run(&a.id).unwrap().session_key, a.session_key);
    assert!(
        s.detail(&next.run_id)
            .unwrap()
            .conversation
            .iter()
            .any(|m| m.text == "원래 답변")
    );
}
#[test]
fn group_edits_reject_cross_project_and_stale_updates_but_allow_idempotent_retry() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("a")).unwrap();
    let b = s.submit(request("b")).unwrap();
    let mut outside = request("outside");
    outside.project_key = "other".into();
    let c = s.submit(outside).unwrap();
    assert!(
        s.set_session_groups(&a.run_id, vec![c.work_id], vec![a.work_id.clone()])
            .is_err()
    );
    let saved = s
        .set_session_groups(
            &a.run_id,
            vec![b.work_id.clone(), a.work_id.clone(), a.work_id.clone()],
            vec![a.work_id.clone()],
        )
        .unwrap();
    assert_eq!(saved.work_ids.len(), 2);
    assert!(
        s.set_session_groups(&a.run_id, vec![], vec![a.work_id.clone()])
            .is_err()
    );
    let seq = s.snapshot().unwrap().last_seq;
    assert_eq!(
        s.set_session_groups(&a.run_id, saved.work_ids.clone(), vec![a.work_id])
            .unwrap(),
        saved
    );
    assert_eq!(s.snapshot().unwrap().last_seq, seq);
    assert_eq!(s.snapshot().unwrap().session_groups, vec![saved]);
}
#[test]
fn external_observations_cannot_be_resolved_as_owned_processes() {
    let s = Store::memory().unwrap();
    setup(&s);
    let a = s.submit(request("a")).unwrap();
    let mut external = s.run(&a.run_id).unwrap();
    external.id = "external".into();
    external.session_id = "external".into();
    external.origin = Origin::External;
    external.state = RunState::Uncertain;
    external.capabilities = Capabilities::external(&Provider::Mock);
    s.import_external(external.clone(), vec![]).unwrap();
    assert!(matches!(
        s.resolve_uncertain(&external.id),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(s.run(&external.id).unwrap().state, RunState::Uncertain);
    s.uncertain(&a.run_id, "owned process uncertain").unwrap();
    assert_eq!(
        s.resolve_uncertain(&a.run_id).unwrap().state,
        RunState::Interrupted
    );
}

#[test]
fn reconciled_subagent_identity_keeps_manual_groups_and_search_without_cloning() {
    let s = Store::memory().unwrap();
    setup(&s);
    let parent = s.submit(request("parent")).unwrap();
    let mut second = request("other-group");
    second.title = Some("연결한그룹검색".into());
    let other = s.submit(second).unwrap();
    let original = s.run(&parent.run_id).unwrap();
    for (i, id) in ["child-old", "child-new"].iter().enumerate() {
        let mut child = original.clone();
        child.id = (*id).into();
        child.session_id = (*id).into();
        child.parent_session_id = Some(original.session_id().into());
        child.agent_kind = "subagent".into();
        child.origin = Origin::External;
        child.capabilities = Capabilities::external(&Provider::Mock);
        child.state = RunState::Completed;
        child.created_at += i as i64;
        s.import_external(child, vec![]).unwrap();
    }
    s.set_session_groups(
        "child-new",
        vec![other.work_id.clone()],
        vec![parent.work_id.clone()],
    )
    .unwrap();
    s.reconcile_subagent_history(
        &parent.run_id,
        "native-child",
        &["child-new".into(), "child-old".into()],
    )
    .unwrap();
    let snap = s.snapshot().unwrap();
    let saved = snap
        .session_groups
        .iter()
        .find(|m| m.session_id == "child-old")
        .unwrap();
    assert_eq!(saved.work_ids.len(), 2);
    assert!(saved.work_ids.contains(&other.work_id));
    assert!(saved.work_ids.contains(&parent.work_id));
    assert_eq!(s.run("child-new").unwrap().session_id(), "child-old");
    assert_eq!(snap.runs.len(), 4);
    let hits = s.search_sessions("p", "연결한그룹검색").unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits.iter()
            .filter(|h| h.run.session_id() == "child-old")
            .count(),
        1
    );
    s.set_session_groups("child-new", vec![], saved.work_ids.clone())
        .unwrap();
    s.reconcile_subagent_history(
        &parent.run_id,
        "native-child",
        &["child-new".into(), "child-old".into()],
    )
    .unwrap();
    assert!(
        s.snapshot()
            .unwrap()
            .session_groups
            .iter()
            .find(|m| m.session_id == "child-old")
            .unwrap()
            .work_ids
            .is_empty()
    );
    assert_eq!(s.search_sessions("p", "연결한그룹검색").unwrap().len(), 1);
}
