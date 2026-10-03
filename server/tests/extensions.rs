use bibi_core::*;
use bibi_server::{
    config::ServiceConfig,
    providers::extensions::{self, Edit},
    runtime::Engine,
};
use serde_json::{Value, json};
fn setup(adapter: &str) -> (Engine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().unwrap();
    for id in ["one", "two"] {
        let path = dir.path().join(id);
        std::fs::create_dir(&path).unwrap();
        store
            .add_project(Project {
                id: id.into(),
                name: id.into(),
                workspace: path.to_string_lossy().into(),
                guild_path: None,
                constraints: vec![],
            })
            .unwrap();
    }
    let provider:ProviderConfig=serde_json::from_value(json!({"id":"p","name":"fixture","host_id":"local","adapter":adapter,"endpoint":"http://127.0.0.1:11434","command":"node","args":[concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/extensions.mjs"),dir.path().join("wire"),"native"]})).unwrap();
    store.save_provider(provider, None).unwrap();
    (
        Engine::new(store, ServiceConfig::new(dir.path().join("data"))),
        dir,
    )
}
async fn edit(
    e: &Engine,
    kind: &str,
    action: &str,
    id: &str,
    value: Value,
) -> anyhow::Result<Value> {
    let view = extensions::list(e, "one", "p").await?;
    extensions::update(
        e,
        "one",
        "p",
        Edit {
            kind: kind.into(),
            action: action.into(),
            id: id.into(),
            revision: view["revision"].as_str().unwrap().into(),
            value,
        },
    )
    .await
}
#[tokio::test]
async fn native_project_files_round_trip_secrets_preserve_other_projects_and_reject_stale_edits() {
    let (e, dir) = setup("codex");
    let root = dir.path().join("one");
    std::fs::create_dir(root.join(".codex")).unwrap();
    let path = root.join(".codex/config.toml");
    std::fs::write(&path, "# keep me\nmodel = 'original'\n").unwrap();
    let before = extensions::list(&e, "one", "p").await.unwrap();
    assert!(!before.to_string().contains("never-expose"));
    edit(
        &e,
        "mcp",
        "add",
        "test",
        json!({"command":"test-mcp","args":["a"],"env":{"TOKEN":"secret-test"}}),
    )
    .await
    .unwrap();
    let view = extensions::list(&e, "one", "p").await.unwrap();
    let item = view["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "mcp" && v["id"] == "test")
        .unwrap();
    assert!(!view.to_string().contains("secret-test"));
    let mut config = item["config"].clone();
    config["args"] = json!(["b"]);
    edit(&e, "mcp", "save", "test", config).await.unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("secret-test"));
    assert!(raw.contains("# keep me"));
    assert!(raw.contains("original"));
    assert!(!dir.path().join("two/.codex").exists());
    let stale = extensions::update(
        &e,
        "one",
        "p",
        Edit {
            kind: "mcp".into(),
            action: "remove".into(),
            id: "test".into(),
            revision: before["revision"].as_str().unwrap().into(),
            value: Value::Null,
        },
    )
    .await
    .unwrap_err();
    assert!(stale.to_string().contains("설정이 변경"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
    edit(&e, "mcp", "toggle", "shared", json!(false))
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("enabled = false")
    );
    edit(&e, "mcp", "remove", "test", Value::Null)
        .await
        .unwrap();
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("secret-test")
    );
    let wire = std::fs::read_to_string(dir.path().join("wire")).unwrap();
    assert!(!wire.contains("turn/start"));
    assert!(!wire.contains("tools/call"));
}
#[tokio::test]
async fn skills_are_project_scoped_and_removal_keeps_supporting_files() {
    let (e, dir) = setup("codex");
    edit(
        &e,
        "skill",
        "add",
        "local",
        json!("---\nname: local\ndescription: sample\n---\nBody"),
    )
    .await
    .unwrap();
    let path = dir
        .path()
        .join("one/.agents/skills/local/SKILL.md")
        .canonicalize()
        .unwrap();
    let id = path.to_string_lossy();
    std::fs::write(path.parent().unwrap().join("support.txt"), "preserve").unwrap();
    edit(&e, "skill", "toggle", &id, json!(false))
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(dir.path().join("one/.codex/config.toml"))
            .unwrap()
            .contains("enabled = false")
    );
    edit(&e, "skill", "save", &id, json!("new content"))
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    edit(&e, "skill", "remove", &id, Value::Null).await.unwrap();
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(path.parent().unwrap().join("support.txt")).unwrap(),
        "preserve"
    );
    assert!(
        edit(&e, "skill", "add", "../escape", json!("bad"))
            .await
            .is_err()
    );
    assert!(!dir.path().join("two/.agents").exists());
}
#[tokio::test]
async fn malformed_config_is_not_overwritten_and_provider_scope_is_honest() {
    let (e, dir) = setup("codex");
    edit(&e, "plugin", "toggle", "shared@example", json!(false))
        .await
        .unwrap();
    let raw = std::fs::read_to_string(dir.path().join("one/.codex/config.toml")).unwrap();
    assert!(raw.contains("shared@example"));
    assert!(
        edit(&e, "plugin", "add", "new@example", Value::Null)
            .await
            .unwrap_err()
            .to_string()
            .contains("사용자 전체")
    );
    let path = dir.path().join("one/.codex/config.toml");
    std::fs::write(&path, "invalid=[").unwrap();
    assert!(extensions::list(&e, "one", "p").await.is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid=[");
    let (e, _) = setup("ollama");
    let result = extensions::list(&e, "one", "p").await.unwrap();
    assert_eq!(result["supported"], false);
    assert!(!result["errors"].as_array().unwrap().is_empty());
}
#[tokio::test]
async fn claude_project_skill_and_plugin_overrides_do_not_change_user_settings() {
    let (e, dir) = setup("claude");
    edit(
        &e,
        "skill",
        "add",
        "fixture-only-test-skill",
        json!("---\ndescription: test\n---\nNo execution"),
    )
    .await
    .unwrap();
    let path = dir
        .path()
        .join("one/.claude/skills/fixture-only-test-skill/SKILL.md")
        .canonicalize()
        .unwrap();
    edit(&e, "skill", "toggle", &path.to_string_lossy(), json!(false))
        .await
        .unwrap();
    edit(&e, "plugin", "toggle", "shared@example", json!(false))
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("one/.claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["skillOverrides"]["fixture-only-test-skill"], "off");
    assert_eq!(settings["enabledPlugins"]["shared@example"], false);
    assert!(!dir.path().join("two/.claude").exists());
    edit(&e, "plugin", "add", "new@example", Value::Null)
        .await
        .unwrap();
    let wire = std::fs::read_to_string(dir.path().join("wire")).unwrap();
    assert!(wire.contains("--scope\",\"project"));
    assert!(!wire.contains("--yes"));
}
#[tokio::test]
async fn mcp_stdio_probe_only_lists_tools_and_terminates_its_process() {
    let (e, dir) = setup("claude");
    let record = dir.path().join("mcp-wire");
    edit(&e,"mcp","add","fixture-mcp-test",json!({"command":"node","args":[concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/extensions.mjs"),record,"mcp"]})).await.unwrap();
    let result = extensions::check_mcp(&e, "one", "p", "fixture-mcp-test")
        .await
        .unwrap();
    assert_eq!(result["tools"].as_array().unwrap().len(), 2);
    let wire = std::fs::read_to_string(record).unwrap();
    assert!(wire.contains("initialize"));
    assert!(wire.contains("tools/list"));
    assert!(!wire.contains("tools/call"));
    assert!(!wire.contains("turn/start"));
}
#[cfg(unix)]
#[tokio::test]
async fn symlink_config_cannot_write_outside_the_project() {
    let (e, dir) = setup("codex");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("config.toml"), "model='keep'").unwrap();
    std::os::unix::fs::symlink(&outside, dir.path().join("one/.codex")).unwrap();
    assert!(
        extensions::list(&e, "one", "p")
            .await
            .unwrap_err()
            .to_string()
            .contains("심볼릭")
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("config.toml")).unwrap(),
        "model='keep'"
    );
}

#[tokio::test]
async fn remote_extensions_change_only_the_mapped_project_on_the_execution_host() {
    use bibi_server::{peer, router, state};
    for existing in [false, true] {
        let (central, local) = setup("codex");
        let remote_dir = tempfile::tempdir().unwrap();
        let remote = state(
            Store::memory().unwrap(),
            ServiceConfig::new(remote_dir.path().into()),
            "extensions-fixture-token-at-least-32-characters".into(),
        );
        remote.store.save_provider(serde_json::from_value(json!({"id":"native","name":"remote fixture","adapter":"codex","command":"node","args":[concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/extensions.mjs"),remote_dir.path().join("wire"),"native"]})).unwrap(),None).unwrap();
        if existing {
            remote
                .store
                .add_project(Project {
                    id: "independent-remote-id".into(),
                    name: "Remote original name".into(),
                    workspace: remote_dir
                        .path()
                        .canonicalize()
                        .unwrap()
                        .to_string_lossy()
                        .into(),
                    guild_path: None,
                    constraints: vec!["preserve original constraint".into()],
                })
                .unwrap();
        }
        remote.engine.start().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(axum::serve(listener, router(remote.clone())).into_future());
        let host = peer::register(
            &central,
            "extension host".into(),
            url,
            remote.token.as_ref().clone(),
            "one".into(),
            remote_dir.path().to_string_lossy().into(),
            None,
        )
        .await
        .unwrap();
        let id = remote_provider_id(&host.id, "native");
        let view = extensions::list(&central, "one", &id).await.unwrap();
        assert_eq!(
            view["workspace"],
            json!(remote_dir.path().canonicalize().unwrap())
        );
        extensions::update(
            &central,
            "one",
            &id,
            Edit {
                kind: "skill".into(),
                action: "add".into(),
                id: "remote-test".into(),
                revision: view["revision"].as_str().unwrap().into(),
                value: json!("---\nname: remote-test\ndescription: test\n---\nfixture"),
            },
        )
        .await
        .unwrap();
        assert!(
            remote_dir
                .path()
                .join(".agents/skills/remote-test/SKILL.md")
                .exists()
        );
        assert!(!local.path().join("one/.agents").exists());
        extensions::list(&central, "one", &id).await.unwrap();
        let projects = remote.store.snapshot().unwrap().projects;
        assert_eq!(projects.len(), 1);
        assert_eq!(
            projects[0].id,
            if existing {
                "independent-remote-id"
            } else {
                "one"
            }
        );
        if existing {
            assert_eq!(projects[0].constraints, ["preserve original constraint"]);
        }
        central
            .store
            .set_setting(
                &format!("host_workspace:{}:one", host.id),
                &local.path().join("two").to_string_lossy(),
            )
            .unwrap();
        let err = extensions::list(&central, "one", &id).await.unwrap_err();
        assert!(err.to_string().contains("작업 경로가 변경"));
        assert!(!local.path().join("two/.agents").exists());
        remote.engine.stop().await;
        server.abort();
    }
}
