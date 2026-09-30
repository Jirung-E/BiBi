use bibi_core::*;
use serde_json::json;

fn setup() -> Store {
    let store = Store::memory().unwrap();
    store
        .add_project(Project {
            id: "p".into(),
            name: "permissions".into(),
            workspace: "/workspace".into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    store
        .upsert_host(Host {
            id: "local".into(),
            name: "test".into(),
            platform: "fixture".into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![
                Provider::Codex,
                Provider::Claude,
                Provider::Ollama,
                Provider::Mock,
            ],
            error: None,
        })
        .unwrap();
    store
}
fn request(id: &str) -> Submission {
    serde_json::from_value(json!({"submission_id":id,"project_key":"p","question":"fixture only","provider":"codex","host_id":"local","role":"coordinator","mode":"fresh","read_only":false})).unwrap()
}
fn finish(store: &Store, receipt: &Receipt) -> Run {
    let run = store.claim_next("local").unwrap().unwrap();
    assert_eq!(run.id, receipt.run_id);
    store
        .delivered(&run.id, "same-native-session", &format!("turn-{}", run.id))
        .unwrap();
    store
        .complete(&run.id, "fixture", UsageStats::default())
        .unwrap();
    store.run(&run.id).unwrap()
}
fn follow(run: &Run, id: &str) -> Submission {
    let mut next = request(id);
    next.mode = SubmitMode::Continue;
    next.target_run_id = Some(run.id.clone());
    next.expected_context_revision = Some(run.context_revision);
    next.expected_turn_id = run.turn_id.clone();
    next.work_id = Some(run.work_id.clone());
    next.read_only = run.read_only;
    next
}
#[test]
fn approval_modes_keep_legacy_wire_and_inherit_only_the_same_conversation() {
    let store = setup();
    let old = request("legacy");
    assert!(old.approval_mode.is_none());
    assert!(
        serde_json::to_value(&old)
            .unwrap()
            .get("approval_mode")
            .is_none()
    );
    let receipt = store.submit(old.clone()).unwrap();
    assert_eq!(store.submit(old).unwrap().run_id, receipt.run_id);
    let first = finish(&store, &receipt);
    let old_run = serde_json::to_value(&first).unwrap();
    assert!(old_run.get("approval_mode").is_none());
    assert_eq!(
        serde_json::from_value::<Run>(old_run)
            .unwrap()
            .approval_mode,
        ApprovalMode::OnRequest
    );
    let mut next = follow(&first, "explicit-full");
    next.approval_mode = Some(ApprovalMode::FullAccess);
    let receipt = store.submit(next.clone()).unwrap();
    assert_eq!(store.submit(next.clone()).unwrap().run_id, receipt.run_id);
    next.approval_mode = Some(ApprovalMode::OnRequest);
    assert!(matches!(store.submit(next), Err(Error::Conflict(_))));
    let full = finish(&store, &receipt);
    assert_eq!(full.session_id, first.session_id);
    assert_eq!(full.approval_mode, ApprovalMode::FullAccess);
    let inherited = finish(
        &store,
        &store.submit(follow(&full, "omitted-keeps-full")).unwrap(),
    );
    assert_eq!(inherited.approval_mode, ApprovalMode::FullAccess);
    let mut next = follow(&inherited, "explicit-restrict");
    next.approval_mode = Some(ApprovalMode::OnRequest);
    let restricted = finish(&store, &store.submit(next).unwrap());
    assert_eq!(restricted.approval_mode, ApprovalMode::OnRequest);
    let mut fresh = follow(&restricted, "new-safe-default");
    fresh.mode = SubmitMode::Fresh;
    let fresh = store.submit(fresh).unwrap();
    assert_eq!(
        store.run(&fresh.run_id).unwrap().approval_mode,
        ApprovalMode::OnRequest
    );
    assert_ne!(
        store.run(&fresh.run_id).unwrap().session_id,
        restricted.session_id
    );
}
#[test]
fn approval_modes_reject_unsupported_read_only_and_steer_changes() {
    let store = setup();
    for (provider, mode, read_only) in [
        (Provider::Codex, ApprovalMode::AcceptEdits, false),
        (Provider::Ollama, ApprovalMode::FullAccess, false),
        (Provider::Claude, ApprovalMode::FullAccess, true),
        (Provider::Mock, ApprovalMode::FullAccess, false),
    ] {
        let mut input = request("invalid");
        input.provider = provider;
        input.approval_mode = Some(mode);
        input.read_only = read_only;
        assert!(store.submit(input).is_err());
    }
    assert!(store.snapshot().unwrap().runs.is_empty());
    let mut input = request("full-active");
    input.approval_mode = Some(ApprovalMode::FullAccess);
    let receipt = store.submit(input).unwrap();
    let run = store.claim_next("local").unwrap().unwrap();
    let active = store.delivered(&run.id, "native", "turn").unwrap();
    let mut input = follow(&active, "steer-keeps-mode");
    input.mode = SubmitMode::Steer;
    assert_eq!(store.submit(input).unwrap().run_id, receipt.run_id);
    let mut input = follow(&active, "steer-cannot-change");
    input.mode = SubmitMode::Steer;
    input.approval_mode = Some(ApprovalMode::OnRequest);
    assert!(matches!(store.submit(input), Err(Error::Conflict(_))));
    assert_eq!(
        store.run(&run.id).unwrap().approval_mode,
        ApprovalMode::FullAccess
    );
}
