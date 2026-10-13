//! Cross-project native history index. Discovery does not create projects or run a model.
mod history;
use crate::{
    providers::{claude_live, claude_usage, imports},
    runtime::Engine,
};
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub provider: Provider,
    pub provider_id: Option<String>,
    pub provider_name: String,
    pub workspace: String,
    pub project_key: Option<String>,
    pub project_name: String,
    pub host_id: String,
    pub native_id: Option<String>,
    pub run_id: Option<String>,
    pub state: String,
    pub updated_at: i64,
    pub excerpt: String,
    pub last_user: String,
    pub last_assistant: String,
    pub terminal_supported: bool,
    pub terminal_reason: Option<String>,
    pub external: bool,
    #[serde(skip)]
    pub(crate) root: PathBuf,
    #[serde(skip)]
    pub(crate) file: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    native_id: String,
    workspace: String,
    title: String,
    model: String,
    updated_at: i64,
    last_user: String,
    last_assistant: String,
    text: String,
    truncated: bool,
}
#[derive(Serialize)]
pub struct Listing {
    pub items: Vec<Entry>,
    pub warnings: Vec<String>,
    pub has_more: bool,
}
pub(crate) fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
fn default_root(engine: &Engine, provider: &Provider) -> Result<PathBuf> {
    if let Some(root) = if *provider == Provider::Claude {
        &engine.config.claude_home
    } else {
        &engine.config.codex_home
    } {
        return Ok(root.clone());
    }
    if *provider == Provider::Claude {
        claude_usage::config_directory()
    } else {
        Ok(std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or(
            dirs::home_dir()
                .context("사용자 홈을 찾지 못했습니다.")?
                .join(".codex"),
        ))
    }
}
pub(crate) fn root_from_file(file: &Path, provider: &Provider) -> Option<PathBuf> {
    file.ancestors()
        .find(|p| {
            p.file_name().is_some_and(|n| {
                if *provider == Provider::Claude {
                    n == "projects"
                } else {
                    n == "sessions" || n == "archived_sessions"
                }
            })
        })
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}
fn state_label(state: &RunState) -> String {
    serde_json::to_value(state)
        .unwrap()
        .as_str()
        .unwrap()
        .into()
}
fn native_identity(provider: &Provider, native: &str, root: &Path) -> String {
    imports::key(&[&format!("{provider:?}"), native, &root.to_string_lossy()])
}
fn stored_entry(engine: &Engine, snapshot: &Snapshot, r: Run, snippet: String) -> Entry {
    let provider = snapshot
        .providers
        .iter()
        .find(|p| Some(&p.id) == r.provider_id.as_ref());
    let mut reason = if r.host_id != "local" {
        Some("세션이 저장된 호스트에서 여세요.")
    } else if r.agent_kind == "subagent" || r.parent_session_id.is_some() {
        Some("부모 세션에서 이어가세요.")
    } else if !matches!(r.provider, Provider::Claude | Provider::Codex) {
        Some("이 제공자는 네이티브 CLI 이어가기를 지원하지 않습니다.")
    } else if provider.is_none() {
        Some("원래 제공자 연결을 복원하세요.")
    } else if r.session_key.is_none() {
        Some("원본 세션 ID가 없습니다.")
    } else {
        None
    };
    if r.origin == Origin::Managed
        && (r.state.active()
            || matches!(
                r.state,
                RunState::Queued | RunState::Uncertain | RunState::Disconnected
            ))
    {
        reason = Some("현재 실행·승인·대기 메시지를 먼저 정리하세요.");
    }
    Entry {
        id: format!("run:{}", r.id),
        title: r.title.clone(),
        provider: r.provider.clone(),
        provider_id: r.provider_id.clone(),
        provider_name: provider
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("{:?}", r.provider)),
        workspace: r.workspace.clone(),
        project_key: Some(r.project_key.clone()),
        project_name: snapshot
            .projects
            .iter()
            .find(|p| p.id == r.project_key)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        host_id: r.host_id.clone(),
        native_id: r.session_key.clone(),
        run_id: Some(r.id.clone()),
        state: state_label(&r.state),
        updated_at: r.updated_at,
        excerpt: snippet,
        last_user: String::new(),
        last_assistant: String::new(),
        terminal_supported: reason.is_none(),
        terminal_reason: reason.map(str::to_string),
        external: r.origin == Origin::External,
        root: r
            .runtime
            .session_file
            .as_ref()
            .and_then(|s| root_from_file(Path::new(s), &r.provider))
            .or_else(|| default_root(engine, &r.provider).ok())
            .unwrap_or_default(),
        file: r
            .runtime
            .session_file
            .map(PathBuf::from)
            .unwrap_or_default(),
    }
}
pub fn list(
    engine: &Engine,
    query: &str,
    provider_filter: Option<&str>,
    project_filter: Option<&str>,
) -> Result<Listing> {
    let query = query.trim().to_lowercase();
    if query.chars().count() > 160 {
        bail!("검색어는 160자 이하로 입력하세요.");
    }
    let snapshot = engine.store.snapshot()?;
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    let mut known = HashMap::<String, Run>::new();
    let mut hidden = HashSet::new();
    let roots_for = |p: &ProviderConfig| -> Vec<PathBuf> {
        let mut roots = default_root(engine, &p.adapter)
            .ok()
            .into_iter()
            .collect::<Vec<_>>();
        for r in snapshot
            .runs
            .iter()
            .chain(&snapshot.removed_sessions)
            .filter(|r| r.provider_id.as_deref() == Some(&p.id) && r.host_id == "local")
        {
            if let Some(root) = r
                .runtime
                .session_file
                .as_ref()
                .and_then(|s| root_from_file(Path::new(s), &p.adapter))
            {
                roots.push(root);
            }
        }
        roots = roots
            .into_iter()
            .map(|r| dunce::canonicalize(&r).unwrap_or(r))
            .collect();
        roots.sort();
        roots.dedup();
        roots
    };
    for r in snapshot.runs.iter().chain(&snapshot.removed_sessions) {
        if let Some(native) = &r.session_key {
            let root = r
                .runtime
                .session_file
                .as_ref()
                .and_then(|s| root_from_file(Path::new(s), &r.provider))
                .or_else(|| default_root(engine, &r.provider).ok())
                .unwrap_or_default();
            let identity = native_identity(
                &r.provider,
                native,
                &dunce::canonicalize(&root).unwrap_or(root),
            );
            if r.host_id == "local" {
                let previous = known.entry(identity.clone()).or_insert_with(|| r.clone());
                if (r.origin == Origin::Managed, r.updated_at, &r.id)
                    > (
                        previous.origin == Origin::Managed,
                        previous.updated_at,
                        &previous.id,
                    )
                {
                    *previous = r.clone();
                }
            }
            if r.host_id == "local"
                && snapshot
                    .removed_sessions
                    .iter()
                    .any(|h| h.session_id() == r.session_id())
            {
                hidden.insert(identity);
            }
        }
    }
    for hit in engine.store.search_all_sessions(&query)? {
        let r = hit.run;
        if provider_filter.is_some_and(|p| r.provider_id.as_deref() != Some(p))
            || project_filter.is_some_and(|p| r.project_key != p)
        {
            continue;
        }
        items.push(stored_entry(engine, &snapshot, r, hit.excerpt));
    }
    let mut budget = 256 * 1024 * 1024;
    for provider in snapshot.providers.iter().filter(|p| {
        p.host_id == "local"
            && matches!(p.adapter, Provider::Claude | Provider::Codex)
            && provider_filter.is_none_or(|id| p.id == id)
    }) {
        for root in roots_for(provider) {
            let files = match history::files(&root, &provider.adapter) {
                Ok(f) => f,
                Err(e) => {
                    warnings.push(format!("{}: {e}", provider.name));
                    continue;
                }
            };
            let live = if provider.adapter == Provider::Claude {
                match claude_live::registrations(&root) {
                    Ok(v) => v,
                    Err(e) => {
                        warnings.push(format!("{} 실행 상태: {e}", provider.name));
                        HashMap::new()
                    }
                }
            } else {
                HashMap::new()
            };
            let names = if provider.adapter == Provider::Codex {
                match history::names(&root) {
                    Ok(v) => v,
                    Err(e) => {
                        warnings.push(e.to_string());
                        HashMap::new()
                    }
                }
            } else {
                HashMap::new()
            };
            for file in files {
                let cache_key =
                    imports::key(&[&format!("{:?}", provider.adapter), &file.to_string_lossy()]);
                let record = match cached_record(
                    engine,
                    &file,
                    &provider.adapter,
                    &cache_key,
                    &mut budget,
                ) {
                    Ok(record) => record,
                    Err(e) => {
                        if warnings.len() < 20 {
                            warnings.push(format!(
                                "{} · {}: {e}",
                                provider.name,
                                file.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        }
                        continue;
                    }
                };
                let Some(mut record) = record else {
                    continue;
                };
                if let Some(name) = names.get(&record.native_id) {
                    record.title = name.clone();
                }
                let identity = native_identity(&provider.adapter, &record.native_id, &root);
                if hidden.contains(&identity) {
                    continue;
                }
                if record.truncated && warnings.len() < 20 {
                    warnings.push("일부 큰 대화의 본문 검색은 처음 2 MiB로 제한됩니다.".into());
                }
                let project = snapshot
                    .projects
                    .iter()
                    .find(|p| claude_live::same_workspace(&p.workspace, &record.workspace));
                let existing = known.get(&identity).filter(|r| {
                    provider_filter.is_none_or(|p| r.provider_id.as_deref() == Some(p))
                });
                if project_filter.is_some_and(|id| {
                    existing
                        .map(|r| r.project_key.as_str())
                        .or_else(|| project.map(|p| p.id.as_str()))
                        != Some(id)
                }) {
                    continue;
                }
                let metadata = format!(
                    "{} {} {} {} {} {}",
                    record.title,
                    record.workspace,
                    record.native_id,
                    record.model,
                    provider.name,
                    project.map(|p| p.name.as_str()).unwrap_or("")
                );
                let matches = query.is_empty()
                    || metadata.to_lowercase().contains(&query)
                    || record.text.to_lowercase().contains(&query);
                if let Some(run) = existing {
                    let id = format!("run:{}", run.id);
                    let index = items.iter().position(|e| e.id == id);
                    if !matches && index.is_none() {
                        continue;
                    }
                    // Update the preview of an already imported history without changing ownership or importing messages.
                    if !claude_live::same_workspace(&run.workspace, &record.workspace) {
                        continue;
                    }
                    let index = index.unwrap_or_else(|| {
                        items.push(stored_entry(
                            engine,
                            &snapshot,
                            run.clone(),
                            excerpt(&record.text, &query),
                        ));
                        items.len() - 1
                    });
                    let item = &mut items[index];
                    item.last_user = record.last_user;
                    item.last_assistant = record.last_assistant;
                    item.updated_at = item.updated_at.max(record.updated_at);
                    if matches {
                        item.excerpt = excerpt(&record.text, &query);
                    }
                    continue;
                }
                if !matches || !seen.insert(identity.clone()) {
                    continue;
                }

                let current = live.get(&record.native_id);
                let state = current
                    .map(|l| state_label(&l.state))
                    .unwrap_or_else(|| "disconnected".into());
                let reason = if !Path::new(&record.workspace).is_dir() {
                    Some("원래 작업 폴더가 없습니다.")
                } else {
                    None
                };
                items.push(Entry {
                    id: format!("native:{}:{identity}", provider.id),
                    title: record.title,
                    provider: provider.adapter.clone(),
                    provider_id: Some(provider.id.clone()),
                    provider_name: provider.name.clone(),
                    workspace: record.workspace,
                    project_key: project.map(|p| p.id.clone()),
                    project_name: project
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "외부 프로젝트".into()),
                    host_id: "local".into(),
                    native_id: Some(record.native_id),
                    run_id: None,
                    state,
                    updated_at: record.updated_at,
                    excerpt: excerpt(&record.text, &query),
                    last_user: record.last_user,
                    last_assistant: record.last_assistant,
                    terminal_supported: reason.is_none(),
                    terminal_reason: reason.map(str::to_string),
                    external: true,
                    root: root.clone(),
                    file,
                });
            }
        }
    }
    items.sort_by(|a, b| (b.updated_at, &b.id).cmp(&(a.updated_at, &a.id)));
    let mut unique = HashSet::new();
    items.retain(|e| {
        let key = if let Some(native) = &e.native_id {
            format!(
                "{}:{}",
                e.host_id,
                native_identity(
                    &e.provider,
                    native,
                    &dunce::canonicalize(&e.root).unwrap_or(e.root.clone())
                )
            )
        } else {
            e.id.clone()
        };
        unique.insert(key)
    });
    let has_more = items.len() > 100;
    items.truncate(100);
    for entry in &mut items {
        if entry.last_user.is_empty()
            && entry.last_assistant.is_empty()
            && let Some(id) = &entry.run_id
        {
            let detail = engine.store.detail(id)?;
            let messages = if detail.conversation.is_empty() {
                &detail.messages
            } else {
                &detail.conversation
            };
            let last = |role: &str| {
                messages
                    .iter()
                    .rev()
                    .find(|m| m.role == role)
                    .map(|m| clip(&m.text, 4000))
                    .unwrap_or_default()
            };
            entry.last_user = last("user");
            entry.last_assistant = last("assistant");
        }
    }
    warnings.sort();
    warnings.dedup();
    Ok(Listing {
        items,
        warnings,
        has_more,
    })
}
fn cached_record(
    engine: &Engine,
    file: &Path,
    provider: &Provider,
    key: &str,
    budget: &mut u64,
) -> Result<Option<Record>> {
    let meta = fs::symlink_metadata(file)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        bail!("일반 대화 파일이 아닙니다.");
    }
    let modified = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos()
        .to_string();
    if let Some(v) = engine
        .store
        .catalog_cache(key)?
        .filter(|v| v["version"] == 2 && v["size"] == meta.len() && v["modified"] == modified)
        && let Ok(record) = serde_json::from_value::<Option<Record>>(v["record"].clone())
    {
        return Ok(record);
    }
    let record = history::read(file, provider, budget)?;
    let after = fs::metadata(file)?;
    if meta.len() == after.len() && meta.modified()? == after.modified()? {
        engine.store.save_catalog_cache(
            key,
            &json!({"version":2,"size":meta.len(),"modified":modified,"record":record}),
        )?;
    }
    Ok(record)
}
fn excerpt(text: &str, query: &str) -> String {
    let lower = text.to_lowercase();
    let offset = lower
        .find(query)
        .map(|p| lower[..p].chars().count())
        .unwrap_or(0)
        .saturating_sub(35);
    text.chars().skip(offset).take(180).collect()
}
pub fn find(engine: &Engine, id: &str) -> Result<Entry> {
    // Native IDs are also indexed as metadata. A targeted query bypasses the
    // first 100 recent entries without accepting a caller-supplied file path.
    if let Some(run) = id.strip_prefix("run:") {
        let r = engine.store.run(run)?;
        return list(
            engine,
            r.session_key.as_deref().unwrap_or(r.session_id()),
            None,
            None,
        )?
        .items
        .into_iter()
        .find(|e| e.id == id)
        .context("검색 가능한 세션이 없습니다.");
    }
    // The entry key is opaque; recover the ID from the indexed registered roots.
    let snapshot = engine.store.snapshot()?;
    for p in snapshot
        .providers
        .iter()
        .filter(|p| id.starts_with(&format!("native:{}:", p.id)))
    {
        let mut roots = vec![default_root(engine, &p.adapter)?];
        for r in &snapshot.runs {
            if r.provider_id.as_deref() == Some(&p.id)
                && let Some(root) = r
                    .runtime
                    .session_file
                    .as_ref()
                    .and_then(|s| root_from_file(Path::new(s), &p.adapter))
            {
                roots.push(root);
            }
        }
        for root in roots {
            let root = dunce::canonicalize(&root).unwrap_or(root);
            for path in history::files(&root, &p.adapter)? {
                let key = imports::key(&[&format!("{:?}", p.adapter), &path.to_string_lossy()]);
                if let Some(cache) = engine.store.catalog_cache(&key)?
                    && let Ok(record) = serde_json::from_value::<Record>(cache["record"].clone())
                    && id
                        == format!(
                            "native:{}:{}",
                            p.id,
                            native_identity(&p.adapter, &record.native_id, &root)
                        )
                {
                    return list(engine, &record.native_id, Some(&p.id), None)?
                        .items
                        .into_iter()
                        .find(|e| {
                            e.id == id
                                || (e.native_id.as_deref() == Some(record.native_id.as_str())
                                    && native_identity(
                                        &e.provider,
                                        &record.native_id,
                                        &dunce::canonicalize(&e.root).unwrap_or(e.root.clone()),
                                    ) == native_identity(&p.adapter, &record.native_id, &root))
                        })
                        .context("세션이 이동되거나 제거됐습니다. 목록을 새로고침하세요.");
                }
            }
        }
    }
    bail!("세션을 찾을 수 없습니다. 목록을 새로고침하세요.")
}
pub async fn open_in_bibi(engine: &Engine, id: &str) -> Result<Run> {
    let entry = find(engine, id)?;
    if let Some(id) = entry.run_id {
        return Ok(engine.store.run(&id)?);
    }
    verify(&entry)?;
    let configured = engine.configured(entry.provider_id.as_deref().context("제공자 없음")?)?;
    let project = if let Some(id) = entry.project_key {
        engine.store.project(&id)?
    } else {
        engine.store.add_project(Project {
            id: bibi_core::id("project"),
            name: Path::new(&entry.workspace)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            workspace: entry.workspace.clone(),
            guild_path: None,
            constraints: vec![],
        })?
    };
    if entry.provider == Provider::Claude {
        crate::providers::claude_history::import_one(
            &configured,
            &project,
            &entry.file,
            &entry.root,
        )
    } else {
        let mut configured = configured;
        configured.config.codex_home = Some(entry.root);
        let mut rpc = crate::providers::rpc::Rpc::connect(&configured.config).await?;
        let result = crate::providers::codex::import(
            &configured,
            &project,
            &mut rpc,
            entry.native_id.as_deref().unwrap(),
        )
        .await;
        let _ = rpc.stop().await;
        result?;
        engine
            .store
            .search_sessions(&project.id, entry.native_id.as_deref().unwrap())?
            .into_iter()
            .find(|h| h.run.session_key == entry.native_id)
            .map(|h| h.run)
            .context("가져온 대화를 찾지 못했습니다.")
    }
}

pub(crate) fn verify(entry: &Entry) -> Result<()> {
    if entry.host_id != "local" || !matches!(entry.provider, Provider::Claude | Provider::Codex) {
        bail!("이 호스트의 Claude/Codex 대화만 터미널로 열 수 있습니다.");
    }
    let native = entry
        .native_id
        .as_deref()
        .context("원본 세션 ID가 없습니다.")?;
    uuid::Uuid::parse_str(native)?;
    if !Path::new(&entry.workspace).is_dir() {
        bail!("원래 작업 폴더가 없습니다. 폴더를 복원한 뒤 다시 여세요.");
    }
    if !fs::symlink_metadata(&entry.file).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
    {
        bail!("원본 대화 파일을 확인할 수 없습니다.");
    }
    let root = dunce::canonicalize(&entry.root)?;
    let path = dunce::canonicalize(&entry.file)?;
    if !path.starts_with(&root) {
        bail!("원본 대화가 설정 폴더 밖으로 이동됐습니다.");
    }
    let record = history::read(&path, &entry.provider, &mut (128 * 1024 * 1024))?
        .context("재개할 원본 루트 대화가 아닙니다.")?;
    if record.native_id != native
        || !claude_live::same_workspace(&record.workspace, &entry.workspace)
    {
        bail!("원본 대화 ID 또는 작업 폴더가 변경됐습니다.");
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
