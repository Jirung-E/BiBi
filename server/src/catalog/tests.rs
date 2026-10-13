use super::*;
use crate::config::ServiceConfig;

pub(crate) struct Fixture {
    pub engine: Engine,
    pub dir: tempfile::TempDir,
    pub root: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("profile with space ' $()");
        let mut config = ServiceConfig::new(dir.path().join("data"));
        config.claude_home = Some(root.clone());
        config.codex_home = Some(root.clone());
        config.prepare().unwrap();
        let engine = Engine::new(
            Store::open(config.data_dir.join("bibi.sqlite3")).unwrap(),
            config,
        );
        for (id, adapter) in [("claude", Provider::Claude), ("codex", Provider::Codex)] {
            engine
                .store
                .save_provider(
                    ProviderConfig {
                        id: id.into(),
                        name: id.into(),
                        adapter,
                        host_id: "local".into(),
                        remote_id: None,
                        command: std::env::current_exe().unwrap().to_string_lossy().into(),
                        args: vec![],
                        endpoint: String::new(),
                        models: vec![],
                        api_key_set: false,
                        quota_source: None,
                        ollama: None,
                    },
                    None,
                )
                .unwrap();
        }
        Self { engine, dir, root }
    }
    pub fn workspace(&self, name: &str) -> PathBuf {
        let p = self.dir.path().join(name);
        fs::create_dir_all(&p).unwrap();
        p
    }
    pub fn claude(&self, cwd: &Path, needle: &str) -> (String, PathBuf) {
        let native = uuid::Uuid::new_v4().to_string();
        let path = self
            .root
            .join("projects")
            .join("project")
            .join(format!("{native}.jsonl"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let rows = [
            json!({"type":"user","sessionId":native,"uuid":"u","cwd":cwd,"message":{"content":needle},"timestamp":"2026-10-12T01:00:00Z"}),
            json!({"type":"system","uuid":"s","parentUuid":"u","sessionId":native,"subtype":"compact_boundary"}),
            json!({"type":"assistant","sessionId":native,"uuid":"a","parentUuid":"s","cwd":cwd,"message":{"content":[{"type":"thinking","thinking":"PRIVATE_REASONING"},{"type":"tool_use","input":{"text":"PRIVATE_TOOL"}},{"type":"text","text":"saved answer"}]},"timestamp":"2026-10-12T02:00:00Z"}),
        ];
        fs::write(
            &path,
            rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        (native, path)
    }
    pub fn codex(&self, cwd: &Path) -> (String, PathBuf) {
        let native = uuid::Uuid::new_v4().to_string();
        let path = self
            .root
            .join("sessions/2026/10/13")
            .join(format!("rollout-{native}.jsonl"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let rows = [
            json!({"type":"session_meta","payload":{"id":native,"cwd":cwd,"source":"cli"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"codex history"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"codex answer"}]},"timestamp":"2026-10-12T03:00:00Z"}),
        ];
        fs::write(
            &path,
            rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        (native, path)
    }
}
#[test]
fn discovers_other_folders_without_importing_or_exposing_private_blocks() {
    let f = Fixture::new();
    let a = f.workspace("A");
    let b = f.workspace("B");
    f.claude(&a, "unique context");
    f.codex(&b);
    let all = list(&f.engine, "", None, None).unwrap();
    assert_eq!(all.items.len(), 2);
    assert!(all.items.iter().all(|e| e.run_id.is_none()));
    let found = list(&f.engine, "unique context", None, None).unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].last_user, "unique context");
    assert_eq!(found.items[0].last_assistant, "saved answer");
    for secret in ["PRIVATE_REASONING", "PRIVATE_TOOL"] {
        assert!(
            list(&f.engine, secret, None, None)
                .unwrap()
                .items
                .is_empty()
        );
    }
    assert!(f.engine.store.snapshot().unwrap().projects.is_empty());
    assert!(f.engine.store.snapshot().unwrap().runs.is_empty());
    assert_eq!(
        list(&f.engine, "", Some("codex"), None)
            .unwrap()
            .items
            .len(),
        1
    );
}
#[tokio::test]
async fn selected_import_is_idempotent_and_changed_external_text_is_searchable() {
    let f = Fixture::new();
    let a = f.workspace("A");
    let (native, path) = f.claude(&a, "original question");
    f.claude(&f.workspace("B"), "not imported");
    let item = list(&f.engine, &native, None, None)
        .unwrap()
        .items
        .remove(0);
    let run = open_in_bibi(&f.engine, &item.id).await.unwrap();
    assert_eq!(run.session_key.as_deref(), Some(native.as_str()));
    let row = json!({"type":"assistant","uuid":"a2","parentUuid":"a","sessionId":native,"cwd":a,"message":{"content":"newly written outside BiBi"},"timestamp":"2026-10-13T03:00:00Z"});
    use std::io::Write;
    writeln!(
        fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{row}"
    )
    .unwrap();
    let items = list(&f.engine, "newly written", None, None).unwrap().items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].run_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(items[0].last_assistant, "newly written outside BiBi");
    let again = open_in_bibi(&f.engine, &item.id).await.unwrap();
    assert_eq!(again.id, run.id);
    let snapshot = f.engine.store.snapshot().unwrap();
    assert_eq!(snapshot.projects.len(), 1);
    assert_eq!(snapshot.runs.len(), 1);
    f.engine.store.set_session_hidden(&run.id, true).unwrap();
    assert!(
        list(&f.engine, &native, None, None)
            .unwrap()
            .items
            .is_empty()
    );
}
#[test]
fn malformed_deleted_and_sidechain_records_do_not_hide_other_sessions() {
    let f = Fixture::new();
    let cwd = f.workspace("A");
    f.claude(&cwd, "good record");
    let (_, bad) = f.claude(&cwd, "bad");
    fs::write(&bad, "not json\n").unwrap();
    let (id, side) = f.claude(&cwd, "side");
    fs::write(
        side,
        format!("{}\n", json!({"isSidechain":true,"sessionId":id})),
    )
    .unwrap();
    let result = list(&f.engine, "", None, None).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.warnings.len(), 1);
    fs::remove_file(bad).unwrap();
    assert!(list(&f.engine, "", None, None).unwrap().warnings.is_empty());
}
#[test]
fn native_source_is_revalidated_and_missing_workspace_is_explicit() {
    let f = Fixture::new();
    let cwd = f.workspace("A");
    let (id, path) = f.claude(&cwd, "original");
    let entry = list(&f.engine, &id, None, None).unwrap().items.remove(0);
    verify(&entry).unwrap();
    let data = fs::read_to_string(&path).unwrap();
    fs::write(&path, data.replace(&id, &uuid::Uuid::new_v4().to_string())).unwrap();
    assert!(verify(&entry).is_err());
    fs::write(&path, data).unwrap();
    fs::remove_dir(cwd).unwrap();
    let entry = list(&f.engine, &id, None, None).unwrap().items.remove(0);
    assert!(!entry.terminal_supported);
    assert!(entry.terminal_reason.unwrap().contains("폴더"));
}
#[test]
fn interrupted_tail_and_utf16_surrogates_keep_complete_history() {
    let f = Fixture::new();
    let cwd = f.workspace("A");
    let (id, path) = f.claude(&cwd, "question");
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file,"{{\"type\":\"assistant\",\"uuid\":\"a2\",\"parentUuid\":\"a\",\"sessionId\":\"{id}\",\"message\":{{\"content\":\"answer \\ud800\"}}}}").unwrap();
    write!(file, "{{\"type\":\"assistant\"").unwrap();
    let result = list(&f.engine, "question", None, None).unwrap();
    assert_eq!(result.items.len(), 1);
    assert!(result.items[0].last_assistant.starts_with("answer"));
}
#[cfg(unix)]
#[test]
fn symlinked_history_is_not_discovered_or_launched() {
    let f = Fixture::new();
    let cwd = f.workspace("A");
    let (id, path) = f.claude(&cwd, "original");
    let entry = list(&f.engine, &id, None, None).unwrap().items.remove(0);
    let backup = f.dir.path().join("elsewhere");
    fs::rename(&path, &backup).unwrap();
    std::os::unix::fs::symlink(backup, path).unwrap();
    assert!(list(&f.engine, "", None, None).unwrap().items.is_empty());
    assert!(verify(&entry).is_err());
}

#[test]
fn codex_user_names_and_archived_sessions_are_searchable() {
    let f = Fixture::new();
    let cwd = f.workspace("B");
    let (native, path) = f.codex(&cwd);
    fs::write(
        f.root.join("session_index.jsonl"),
        format!(
            "{}\n",
            json!({"id":native,"thread_name":"renamed archive","updated_at":"2026-10-13T00:00:00Z"})
        ),
    )
    .unwrap();
    let archive = f.root.join("archived_sessions");
    fs::create_dir_all(&archive).unwrap();
    fs::rename(path, archive.join(format!("{native}.jsonl"))).unwrap();
    let items = list(&f.engine, "renamed archive", None, None)
        .unwrap()
        .items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].native_id.as_deref(), Some(native.as_str()));
    assert_eq!(items[0].title, "renamed archive");
    verify(&items[0]).unwrap();
}
#[tokio::test]
async fn catalog_and_selected_import_use_the_existing_authenticated_router() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let f = Fixture::new();
    let (native, _) = f.claude(&f.workspace("A"), "router check");
    let state = crate::state(
        f.engine.store.clone(),
        f.engine.config.clone(),
        "fixture-token".into(),
    );
    let app = crate::router(state);
    let unauth = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sessions/catalog")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/catalog?q={native}"))
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let data: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap();
    let body = json!({"type":"open_catalog_session","id":data["items"][0]["id"]});
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/command")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let data: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap();
    assert_eq!(data["session_key"], native);
}
