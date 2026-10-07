use bibi_core::*;
use serde_json::json;

fn setup(store: &Store) {
    store
        .add_project(Project {
            id: "p".into(),
            name: "dispatch fixture".into(),
            workspace: "/shared".into(),
            guild_path: None,
            constraints: vec![],
        })
        .unwrap();
    store
        .upsert_host(Host {
            id: "local".into(),
            name: "local".into(),
            platform: "fixture".into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![Provider::Claude, Provider::Codex],
            error: None,
        })
        .unwrap();
}
fn request(id: &str) -> Submission {
    serde_json::from_value(json!({"submission_id":id,"project_key":"p","question":id,
        "provider":"claude","host_id":"local","role":"coordinator","mode":"fresh","read_only":false})).unwrap()
}
fn previous(store: &Store) -> Run {
    let receipt = store.submit(request("previous")).unwrap();
    assert_eq!(
        store.claim_next("local").unwrap().unwrap().id,
        receipt.run_id
    );
    store
        .delivered(&receipt.run_id, "native-original", "turn-one")
        .unwrap();
    store
        .complete(&receipt.run_id, "previous answer", UsageStats::default())
        .unwrap();
    store.run(&receipt.run_id).unwrap()
}
fn follow(store: &Store, prior: &Run) -> Receipt {
    let mut submission = request("follow");
    submission.mode = SubmitMode::Continue;
    submission.target_run_id = Some(prior.id.clone());
    submission.expected_turn_id = prior.turn_id.clone();
    submission.expected_context_revision = Some(prior.context_revision);
    submission.read_only = prior.read_only;
    store.submit(submission).unwrap()
}
fn observation(seed: &Run, state: RunState) -> Run {
    let mut run = seed.clone();
    run.id = "external-observation".into();
    run.session_id = run.id.clone();
    run.request_id = "external-request".into();
    run.origin = Origin::External;
    run.capabilities = Capabilities::external(&run.provider);
    run.continued_from = None;
    run.state = state;
    run
}

#[test]
fn independent_writers_share_workspace_and_clear_old_wait_reason() {
    let store = Store::memory().unwrap();
    setup(&store);
    let first = store.submit(request("first")).unwrap();
    store.claim_next("local").unwrap().unwrap();
    let second = store.submit(request("second")).unwrap();
    store
        .observe(
            &second.run_id,
            RunState::Queued,
            "접수됨",
            Some("같은 작업 폴더의 다른 세션이 작업 중입니다.".into()),
        )
        .unwrap();
    assert!(
        store
            .claim_next_excluding("local", std::slice::from_ref(&second.run_id))
            .unwrap()
            .is_none()
    );
    assert_eq!(store.run(&second.run_id).unwrap().wait_reason, None);
    let next = store
        .claim_next("local")
        .unwrap()
        .expect("independent writer must start");
    assert_eq!(next.id, second.run_id);
    assert_eq!(next.workspace, store.run(&first.run_id).unwrap().workspace);
    assert_ne!(
        next.session_id(),
        store.run(&first.run_id).unwrap().session_id()
    );
    assert_eq!(store.run(&first.run_id).unwrap().state, RunState::Running);
    let mut reader = request("reader");
    reader.read_only = true;
    let reader = store.submit(reader).unwrap();
    assert_eq!(
        store.claim_next("local").unwrap().unwrap().id,
        reader.run_id
    );
}

#[test]
fn unrelated_external_states_do_not_block_fresh_or_resumed_work() {
    for state in [
        RunState::Running,
        RunState::WaitingUser,
        RunState::WaitingExpert,
        RunState::Disconnected,
        RunState::Uncertain,
    ] {
        for resume in [false, true] {
            let store = Store::memory().unwrap();
            setup(&store);
            let prior = previous(&store);
            let mut observed = observation(&prior, state.clone());
            observed.session_key = Some("other-native-session".into());
            store.import_external(observed, vec![]).unwrap();
            let receipt = if resume {
                follow(&store, &prior)
            } else {
                store.submit(request("fresh")).unwrap()
            };
            let run = store
                .claim_next("local")
                .unwrap()
                .expect("unrelated external session must not block");
            assert_eq!(run.id, receipt.run_id, "state={state:?}, resume={resume}");
            assert_eq!(run.wait_reason, None);
            if resume {
                assert_eq!(run.session_id(), prior.session_id());
                assert_eq!(run.session_key, prior.session_key);
            }
        }
    }
}

#[test]
fn same_native_session_waits_for_its_owner_even_with_different_path_or_readonly() {
    for state in [
        RunState::Running,
        RunState::WaitingUser,
        RunState::Disconnected,
        RunState::Uncertain,
    ] {
        for readonly in [false, true] {
            let store = Store::memory().unwrap();
            setup(&store);
            let prior = previous(&store);
            let mut observed = observation(&prior, state.clone());
            observed.workspace = "/different-path".into();
            observed.read_only = readonly;
            store.import_external(observed.clone(), vec![]).unwrap();
            let receipt = follow(&store, &prior);
            assert!(
                store.claim_next("local").unwrap().is_none(),
                "state={state:?}, readonly={readonly}"
            );
            let queued = store.run(&receipt.run_id).unwrap();
            assert_eq!(queued.state, RunState::Queued);
            assert!(
                queued
                    .wait_reason
                    .as_deref()
                    .unwrap()
                    .starts_with("같은 세션의")
            );
            if state == RunState::Uncertain {
                assert!(queued.wait_reason.unwrap().contains("실행 확인"));
            }
            observed.state = RunState::Completed;
            store.import_external(observed, vec![]).unwrap();
            let resumed = store.claim_next("local").unwrap().unwrap();
            assert_eq!(resumed.id, receipt.run_id);
            assert_eq!(resumed.session_id(), prior.session_id());
            assert_eq!(resumed.session_key, prior.session_key);
            assert_eq!(resumed.wait_reason, None);
        }
    }
}

#[test]
fn same_logical_session_is_protected_before_a_native_id_is_known() {
    let store = Store::memory().unwrap();
    setup(&store);
    let receipt = store.submit(request("queued")).unwrap();
    let queued = store.run(&receipt.run_id).unwrap();
    let mut observed = observation(&queued, RunState::Running);
    observed.session_id = queued.session_id().into();
    observed.workspace = "/different-path".into();
    observed.read_only = true;
    assert_eq!(observed.session_key, None);
    store.import_external(observed, vec![]).unwrap();
    assert!(store.claim_next("local").unwrap().is_none());
    assert!(
        store
            .run(&receipt.run_id)
            .unwrap()
            .wait_reason
            .unwrap()
            .starts_with("같은 세션의")
    );
}

#[test]
fn native_identity_is_scoped_to_provider_and_host() {
    for other_host in [false, true] {
        let store = Store::memory().unwrap();
        setup(&store);
        let prior = previous(&store);
        let mut observed = observation(&prior, RunState::Running);
        if other_host {
            observed.host_id = "remote".into();
        } else {
            observed.provider = Provider::Codex;
        }
        store.import_external(observed, vec![]).unwrap();
        let receipt = follow(&store, &prior);
        assert_eq!(
            store.claim_next("local").unwrap().unwrap().id,
            receipt.run_id
        );
    }
}

#[test]
fn simultaneous_dispatchers_start_distinct_sessions_once() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dispatch.sqlite3");
    let store = Store::open(&path).unwrap();
    setup(&store);
    let mut expected = [
        store.submit(request("first")).unwrap().run_id,
        store.submit(request("second")).unwrap().run_id,
    ];
    let gate = Arc::new(Barrier::new(2));
    let connections = [Store::open(&path).unwrap(), Store::open(&path).unwrap()];
    let handles = connections
        .into_iter()
        .map(|store| {
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                store.claim_next("local").unwrap().unwrap().id
            })
        })
        .collect::<Vec<_>>();
    let mut claimed = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    claimed.sort();
    expected.sort();
    assert_eq!(claimed, expected);
    assert!(store.claim_next("local").unwrap().is_none());
    assert!(
        store
            .snapshot()
            .unwrap()
            .runs
            .iter()
            .all(|run| run.state == RunState::Running)
    );
}
