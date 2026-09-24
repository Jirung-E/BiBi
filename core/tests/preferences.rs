use bibi_core::*;
use serde_json::json;

fn profile(id: &str) -> ProviderConfig {
    serde_json::from_value(json!({"id":id,"name":"내 모델","adapter":"mock","models":["m1","m2"]}))
        .unwrap()
}
fn setup(s: &Store) {
    s.add_project(Project {
        id: "p".into(),
        name: "p".into(),
        workspace: "/workspace".into(),
        guild_path: None,
        constraints: vec![],
    })
    .unwrap();
    s.upsert_host(Host {
        id: "local".into(),
        name: "local".into(),
        platform: "fixture".into(),
        kind: "local".into(),
        connected: true,
        observed_at: now(),
        providers: vec![Provider::Mock],
        error: None,
    })
    .unwrap();
}
fn request(id: &str) -> Submission {
    serde_json::from_value(json!({"submission_id":id,"project_key":"p","question":"질문","provider":"mock","provider_id":"test","model":"m1","host_id":"local","role":"coordinator","mode":"fresh","read_only":false})).unwrap()
}
fn complete(s: &Store, id: &str) -> Run {
    assert_eq!(s.claim_next("local").unwrap().unwrap().id, id);
    s.delivered(id, "native-session", "native-turn").unwrap();
    s.complete(id, "답변", UsageStats::default()).unwrap();
    s.run(id).unwrap()
}

#[test]
fn profiles_start_empty_and_secrets_never_enter_snapshots_or_events() {
    let s = Store::memory().unwrap();
    assert!(s.snapshot().unwrap().providers.is_empty());
    let saved = s
        .save_provider(profile("test"), Some("credential-fixture".into()))
        .unwrap();
    assert!(saved.api_key_set);
    assert_eq!(
        s.provider_secret("test").unwrap().as_deref(),
        Some("credential-fixture")
    );
    assert!(
        !serde_json::to_string(&s.snapshot().unwrap())
            .unwrap()
            .contains("credential-fixture")
    );
    assert!(
        !serde_json::to_string(&s.events(0, 500).unwrap())
            .unwrap()
            .contains("credential-fixture")
    );
    s.save_provider(saved, None).unwrap();
    assert!(s.provider("test").unwrap().api_key_set);
    s.save_provider(profile("test"), Some(String::new()))
        .unwrap();
    assert!(s.provider_secret("test").unwrap().is_none());
}

#[test]
fn recent_selection_and_usage_counts_survive_restart_and_retry() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("bibi.sqlite3");
    let s = Store::open(&path).unwrap();
    setup(&s);
    s.save_provider(profile("test"), None).unwrap();
    s.select_model(ModelSelection {
        provider_id: "test".into(),
        model: "m2".into(),
    })
    .unwrap();
    s.submit(request("one")).unwrap();
    s.submit(request("one")).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.model_selection.unwrap().model, "m1");
    assert_eq!(
        snap.model_history
            .iter()
            .find(|h| h.model == "m1")
            .unwrap()
            .uses,
        1
    );
    assert_eq!(
        snap.model_history
            .iter()
            .find(|h| h.model == "m2")
            .unwrap()
            .uses,
        0
    );
}

#[test]
fn session_rename_survives_continuation_and_hidden_sessions_are_recoverable() {
    let s = Store::memory().unwrap();
    setup(&s);
    s.save_provider(profile("test"), None).unwrap();
    let a = s.submit(request("one")).unwrap();
    assert!(s.set_session_hidden(&a.run_id, true).is_err());
    assert!(s.delete_provider("test").is_err());
    let first = complete(&s, &a.run_id);
    s.rename_session(&a.run_id, "변경한 이름").unwrap();
    let mut next = request("two");
    next.mode = SubmitMode::Continue;
    next.target_run_id = Some(a.run_id.clone());
    next.expected_turn_id = first.turn_id;
    next.expected_context_revision = Some(1);
    let b = s.submit(next).unwrap();
    assert_eq!(s.run(&b.run_id).unwrap().title, "변경한 이름");
    complete(&s, &b.run_id);
    s.set_session_hidden(&b.run_id, true).unwrap();
    let snap = s.snapshot().unwrap();
    assert!(snap.runs.is_empty());
    assert_eq!(snap.removed_sessions.len(), 2);
    assert!(!s.detail(&b.run_id).unwrap().conversation.is_empty());
    s.set_session_hidden(&a.run_id, false).unwrap();
    assert_eq!(s.snapshot().unwrap().runs.len(), 2);
    s.delete_provider("test").unwrap();
    let snap = s.snapshot().unwrap();
    assert!(snap.providers.is_empty());
    assert_eq!(snap.runs.len(), 2);
    assert!(snap.model_selection.is_none());
    assert_eq!(snap.model_history.len(), 1);
}

#[test]
fn remote_profiles_are_host_scoped_read_only_and_do_not_export_credentials() {
    let s = Store::memory().unwrap();
    setup(&s);
    let mut p = profile("test");
    p.api_key_set = true;
    s.replace_remote_providers("host-a", vec![p.clone()])
        .unwrap();
    s.replace_remote_providers("host-b", vec![p]).unwrap();
    let id = remote_provider_id("host-a", "test");
    let p = s.provider(&id).unwrap();
    assert_eq!(p.host_id, "host-a");
    assert_eq!(p.remote_id.as_deref(), Some("test"));
    assert!(!p.api_key_set);
    assert!(s.save_provider(p, None).is_err());
    assert_eq!(s.snapshot().unwrap().providers.len(), 2);
    let mut request = request("wrong-host");
    request.provider_id = Some(id);
    assert!(s.submit(request).is_err());
    s.replace_remote_providers("host-a", vec![]).unwrap();
    assert_eq!(s.snapshot().unwrap().providers.len(), 1);
}

#[test]
fn quota_refresh_does_not_nest_connection_ids_or_cross_profiles() {
    let s = Store::memory().unwrap();
    let q = Quota {
        id: "local:Ollama:m1".into(),
        provider: Provider::Ollama,
        provider_id: None,
        account: "fixture".into(),
        host_id: "local".into(),
        model: Some("m1".into()),
        status: "unknown".into(),
        windows: vec![],
        observed_at: None,
        reason: None,
    };
    s.replace_connection_quotas("a", &Provider::Ollama, vec![q.clone()])
        .unwrap();
    s.replace_connection_quotas("b", &Provider::Ollama, vec![q])
        .unwrap();
    let before = s
        .quotas()
        .unwrap()
        .into_iter()
        .find(|q| q.provider_id.as_deref() == Some("a"))
        .unwrap();
    s.replace_connection_quotas("a", &Provider::Ollama, vec![before.clone()])
        .unwrap();
    let quotas = s.quotas().unwrap();
    assert_eq!(quotas.len(), 2);
    assert!(quotas.iter().any(|q| q.id == before.id));
}

#[test]
fn ollama_generation_options_persist_validate_and_can_return_to_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("options.sqlite3");
    let store = Store::open(&path).unwrap();
    let mut p: ProviderConfig = serde_json::from_value(json!({
        "id":"ollama", "name":"Local", "adapter":"ollama", "endpoint":"http://127.0.0.1:11434"
    }))
    .unwrap();
    assert!(p.ollama.is_none());
    p.ollama = Some(OllamaOptions {
        think: Some(false),
        num_predict: Some(2048),
        num_ctx: Some(8192),
    });
    store.save_provider(p.clone(), None).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.provider("ollama").unwrap().ollama, p.ollama);
    for invalid in [
        OllamaOptions {
            num_predict: Some(0),
            ..Default::default()
        },
        OllamaOptions {
            num_predict: Some(-2),
            ..Default::default()
        },
        OllamaOptions {
            num_ctx: Some(0),
            ..Default::default()
        },
    ] {
        let mut changed = p.clone();
        changed.ollama = Some(invalid);
        assert!(store.save_provider(changed, None).is_err());
        assert_eq!(store.provider("ollama").unwrap().ollama, p.ollama);
    }
    p.ollama.as_mut().unwrap().num_predict = Some(-1);
    store.save_provider(p.clone(), None).unwrap();
    p.ollama = None;
    store.save_provider(p.clone(), None).unwrap();
    assert!(store.provider("ollama").unwrap().ollama.is_none());
    p.adapter = Provider::Mock;
    p.ollama = Some(OllamaOptions {
        think: Some(true),
        ..Default::default()
    });
    assert!(store.save_provider(p, None).unwrap().ollama.is_none());
}
