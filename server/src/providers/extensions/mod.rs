//! Project-scoped native provider configuration. No model requests are made here.
mod files;
mod probe;
use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use files::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
    time::Duration,
};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Edit {
    pub kind: String,
    pub action: String,
    pub id: String,
    pub revision: String,
    #[serde(default)]
    pub value: Value,
}
fn lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
fn claude_account() -> Result<PathBuf> {
    let dir = super::claude_usage::config_directory()?;
    Ok(if std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
        dir.join(".claude.json")
    } else {
        dir.parent().context("홈 경로 없음")?.join(".claude.json")
    })
}
fn config_path(root: &Path, provider: &Provider) -> Result<PathBuf> {
    scoped(
        root,
        Path::new(if *provider == Provider::Codex {
            ".codex/config.toml"
        } else {
            ".claude/settings.local.json"
        }),
    )
}
fn mcp_path(root: &Path, provider: &Provider) -> Result<PathBuf> {
    if *provider == Provider::Codex {
        config_path(root, provider)
    } else {
        scoped(root, Path::new(".mcp.json"))
    }
}
fn skill_root(root: &Path, provider: &Provider) -> PathBuf {
    root.join(if *provider == Provider::Codex {
        ".agents/skills"
    } else {
        ".claude/skills"
    })
}
// Rust canonical paths and native CLI paths use different Windows prefixes.
// Compare their lexical forms without resolving links or bypassing scoped().
fn project_relative<'a>(root: &Path, path: &'a Path) -> Option<&'a Path> {
    dunce::simplified(path)
        .strip_prefix(dunce::simplified(root))
        .ok()
}
fn same_skill_path(left: &str, right: &str) -> bool {
    dunce::simplified(Path::new(left)) == dunce::simplified(Path::new(right))
}
fn scope(root: &Path, path: &Path) -> &'static str {
    if project_relative(root, path).is_some() {
        "project"
    } else {
        "inherited"
    }
}
fn digest(root: &Path, provider: &Provider) -> Result<String> {
    let mut parts = BTreeMap::new();
    for path in [config_path(root, provider)?, mcp_path(root, provider)?] {
        parts.insert(path.to_string_lossy().into_owned(), text(&path)?);
    }
    if *provider == Provider::Claude {
        for path in [root.join(".claude/settings.json"), claude_account()?] {
            parts.insert(path.to_string_lossy().into_owned(), text(&path)?);
        }
    }
    let path = skill_root(root, provider);
    if path.exists() {
        for entry in std::fs::read_dir(path)?.take(513) {
            let path = entry?.path().join("SKILL.md");
            if path.is_file() {
                parts.insert(path.to_string_lossy().into_owned(), text(&path)?);
            }
        }
    }
    Ok(revision(&serde_json::to_string(&parts)?))
}
fn merge(base: &mut Value, next: &Value) {
    if let (Some(a), Some(b)) = (base.as_object_mut(), next.as_object()) {
        for (k, v) in b {
            if v.is_object() {
                merge(a.entry(k).or_insert(json!({})), v);
            } else {
                a.insert(k.clone(), v.clone());
            }
        }
    }
}
fn object<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut serde_json::Map<String, Value>> {
    if value.get(key).is_none() {
        value[key] = json!({});
    }
    value[key]
        .as_object_mut()
        .context("설정 항목이 객체가 아닙니다.")
}
fn read_skill(path: &Path) -> Result<(String, String, String)> {
    let body = text(path)?;
    let mut name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned();
    let mut description = String::new();
    if body.starts_with("---") {
        for line in body.lines().skip(1).take_while(|l| l.trim() != "---") {
            if let Some(value) = line.strip_prefix("name:") {
                let value = value.trim();
                name = serde_json::from_str::<String>(value)
                    .unwrap_or_else(|_| value.trim_matches('\'').to_owned());
            }
            if let Some(value) = line.strip_prefix("description:") {
                description = value.trim().trim_matches(['\'', '"']).to_owned();
            }
        }
    }
    Ok((body, name, description))
}
fn scan_skills(base: &Path, root: &Path, settings: &Value, out: &mut Vec<Value>) -> Result<()> {
    if !base.exists() {
        return Ok(());
    }
    for item in std::fs::read_dir(base)?.take(513) {
        let dir = item?.path();
        let name = dir.file_name().and_then(|v| v.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "synced" {
            continue;
        }
        let path = dir.join("SKILL.md");
        if !path.is_file() {
            continue;
        }
        let (content, name, description) = read_skill(&path)?;
        let editable = project_relative(root, &path).is_some_and(|rel| scoped(root, rel).is_ok());
        out.push(json!({"kind":"skill","id":path,"name":name,"description":description,"scope":scope(root,&path),"source":path,"enabled":settings["skillOverrides"][&name]!="off","editable":editable,"removable":editable,"toggleable":true,"content":content,"note":settings["skillOverrides"][&name].as_str().filter(|s|*s!="on"&&*s!="off").unwrap_or("")}));
    }
    if out.len() > 512 {
        bail!("스킬 목록이 너무 큽니다.");
    }
    Ok(())
}
async fn claude_cli(engine: &Engine, root: &Path, args: &[&str]) -> Result<Value> {
    let mut command = super::launch::command(&engine.config.claude_command)?;
    command
        .args(&engine.config.claude_args)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = super::launch::spawn(&mut command)?;
    use tokio::io::AsyncReadExt;
    let mut stdout = child
        .stdout
        .take()
        .context("CLI 출력 없음")?
        .take(LIMIT + 1);
    let mut bytes = vec![];
    let result = tokio::time::timeout(Duration::from_secs(25), async {
        stdout.read_to_end(&mut bytes).await?;
        let status = child.wait().await?;
        Ok::<_, anyhow::Error>(status)
    })
    .await;
    if result.is_err() {
        let _ = child.kill().await;
        bail!("Claude 플러그인 명령 시간 초과. 현재 목록을 새로고침해 상태를 확인하세요.");
    }
    let status = result??;
    if !status.success() {
        bail!(
            "Claude 플러그인 명령이 거절되었습니다. 마켓플레이스·프로젝트 권한을 확인하세요. 설치 명령의 별도 승인이 필요하면 원본 CLI에서 진행하세요."
        );
    }
    if bytes.len() as u64 > LIMIT {
        bail!("CLI 출력 크기 초과");
    }
    serde_json::from_slice(&bytes).context("Claude 플러그인 JSON 조회를 지원하는 CLI가 필요합니다.")
}
fn mcp_entries(
    entries: &mut Vec<Value>,
    root: &Path,
    provider: &Provider,
    values: &Value,
    local: &Value,
    source: &str,
    settings: &Value,
) {
    for (name, config) in values.as_object().into_iter().flatten() {
        let is_local = local.get(name).is_some();
        let approved = *provider == Provider::Codex
            || settings["enableAllProjectMcpServers"] == true
            || settings["enabledMcpjsonServers"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(name)));
        let disabled = config["enabled"] == false
            || settings["disabledMcpjsonServers"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(name)));
        entries.push(json!({"kind":"mcp","id":name,"name":name,"scope":if is_local{"project"}else{"inherited"},"source":if is_local{mcp_path(root,provider).unwrap().to_string_lossy().into_owned()}else{source.into()},"enabled":if disabled{Some(false)}else if is_local&&!approved{None}else{Some(true)},"editable":is_local,"removable":is_local,"toggleable":name!="bibi","config":redact(config),"note":if name=="bibi"{"BiBi 내부 연결 · 편집 불가"}else if is_local&&!approved{"프로젝트 서버 사용 승인 필요"}else{""}}));
    }
}
async fn local_view(engine: &Engine, root: &Path, provider: &Provider) -> Result<Value> {
    let mut entries = vec![];
    let mut errors = vec![];
    let mut notes = vec![];
    let path = config_path(root, provider)?;
    if *provider == Provider::Codex {
        let (_, local) = toml_file(&path)?;
        let mut rpc =
            super::rpc::Rpc::connect_project(&engine.config, &root.to_string_lossy()).await?;
        let native = rpc
            .request("config/read", json!({"cwd":root,"includeLayers":true}))
            .await
            .context("Codex 유효 설정 조회 실패 · CLI 버전과 프로젝트 신뢰를 확인하세요.")?;
        let actual = &native["config"];
        let mut mcps = actual["mcp_servers"].clone();
        if !mcps.is_object() {
            mcps = json!({});
        }
        for (name, config) in local["mcp_servers"].as_object().into_iter().flatten() {
            if mcps.get(name).is_none() {
                mcps[name] = config.clone();
                notes.push(format!("{name}: 저장된 프로젝트 MCP가 유효 설정에 없습니다. 프로젝트 신뢰·관리 정책을 확인하세요."));
            }
        }
        mcp_entries(
            &mut entries,
            root,
            provider,
            &mcps,
            &local["mcp_servers"],
            "Codex 유효 설정",
            &json!({}),
        );
        match rpc
            .request("skills/list", json!({"cwds":[root],"forceReload":true}))
            .await
        {
            Ok(value) => {
                for group in value["data"].as_array().into_iter().flatten() {
                    if group["errors"].as_array().is_some_and(|a| !a.is_empty()) {
                        errors.push("일부 Codex 스킬을 읽지 못했습니다.".into());
                    }
                    for s in group["skills"].as_array().into_iter().flatten() {
                        let Some(p) = s["path"].as_str() else {
                            continue;
                        };
                        let path = Path::new(p);
                        let editable = project_relative(root, path)
                            .is_some_and(|rel| scoped(root, rel).is_ok());
                        let content = if editable { Some(text(path)?) } else { None };
                        entries.push(json!({"kind":"skill","id":p,"name":s["name"],"description":s["description"],"scope":scope(root,path),"source":p,"enabled":s["enabled"].as_bool().unwrap_or(true),"editable":editable,"removable":editable,"toggleable":true,"content":content}));
                    }
                }
            }
            Err(_) => errors.push("Codex 스킬 조회 실패 · CLI 버전과 설정을 확인하세요.".into()),
        }
        match rpc
            .request(
                "plugin/list",
                json!({"cwds":[root],"forceRefetch":false,"marketplaceKinds":["local"]}),
            )
            .await
        {
            Ok(value) => {
                if value["marketplaceLoadErrors"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
                {
                    errors.push("일부 플러그인 마켓플레이스를 읽지 못했습니다.".into());
                }
                for m in value["marketplaces"].as_array().into_iter().flatten() {
                    for p in m["plugins"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|p| p["installed"] == true)
                    {
                        let Some(id) = p["id"].as_str() else { continue };
                        let is_local = local["plugins"].get(id).is_some();
                        entries.push(json!({"kind":"plugin","id":id,"name":p["name"],"scope":if is_local{"project"}else{"inherited"},"source":p["source"],"version":p["localVersion"].as_str().or(p["version"].as_str()),"enabled":p["enabled"],"editable":false,"removable":is_local,"toggleable":p["availability"]!="DISABLED_BY_ADMIN","note":"설치는 사용자 범위 · 활성화 설정은 이 프로젝트에 저장"}));
                    }
                }
            }
            Err(_) => {
                errors.push("Codex 플러그인 조회 미지원 또는 실패 · CLI를 확인하세요.".into())
            }
        }
        notes.push("Codex 플러그인 설치·제거 API는 사용자 전체에 적용됩니다. 여기서는 프로젝트별 사용 여부와 재정의 제거만 지원합니다.".into());
    } else {
        let user = super::claude_usage::config_directory()?;
        let mut settings = json_file(&user.join("settings.json"))?;
        merge(
            &mut settings,
            &json_file(&root.join(".claude/settings.json"))?,
        );
        merge(&mut settings, &json_file(&path)?);
        // Native Claude remains the authority for managed policy and workspace trust.
        notes.push("Claude 로컬 스킬·설치된 플러그인·파일 MCP 기준입니다. 계정 동기화 스킬과 관리 정책의 최종 목록은 원본 CLI에서 확인하세요. 변경은 다음 메시지에서 같은 세션에 반영됩니다.".into());
        let mut parents = vec![];
        let mut current = Some(root);
        while let Some(dir) = current {
            parents.push(dir.to_path_buf());
            if dir.join(".git").exists() {
                break;
            }
            current = dir.parent();
        }
        for dir in parents.into_iter().rev() {
            scan_skills(&dir.join(".claude/skills"), root, &settings, &mut entries)?;
        }
        scan_skills(&user.join("skills"), root, &settings, &mut entries)?;
        match claude_cli(engine, root, &["plugin", "list", "--json"]).await {
            Ok(value) => {
                let list = value
                    .as_array()
                    .or_else(|| value["plugins"].as_array())
                    .context("Claude 플러그인 목록 형식 오류")?;
                for p in list {
                    let Some(id) = p["id"].as_str() else { continue };
                    let enabled = settings["enabledPlugins"][id]
                        .as_bool()
                        .unwrap_or(p["enabled"].as_bool().unwrap_or(false));
                    let is_local = p["scope"] == "project" || p["scope"] == "local";
                    entries.push(json!({"kind":"plugin","id":id,"name":id,"scope":if is_local{"project"}else{"inherited"},"source":p["installPath"],"version":p["version"],"enabled":enabled,"editable":false,"removable":p["scope"]=="project","toggleable":true,"note":if p["scope"]=="local"{"로컬 설치 · 제거는 원본 CLI에서"}else{""}}));
                }
            }
            Err(e) => errors.push(e.to_string()),
        }
        let account = json_file(&claude_account()?)?;
        let project_key = root.to_string_lossy();
        let local = json_file(&mcp_path(root, provider)?)?;
        let mut mcps = account["mcpServers"].clone();
        if !mcps.is_object() {
            mcps = json!({});
        }
        merge(&mut mcps, &local["mcpServers"]);
        merge(
            &mut mcps,
            &account["projects"][project_key.as_ref()]["mcpServers"],
        );
        mcp_entries(
            &mut entries,
            root,
            provider,
            &mcps,
            &local["mcpServers"],
            "Claude 사용자/프로젝트 로컬 설정",
            &settings,
        );
        for e in entries.iter_mut().filter(|e| e["kind"] == "mcp") {
            if account["projects"][project_key.as_ref()]["disabledMcpServers"]
                .as_array()
                .is_some_and(|a| a.contains(&e["id"]))
            {
                e["enabled"] = json!(false);
            }
            if account["projects"][project_key.as_ref()]["mcpServers"]
                .get(e["id"].as_str().unwrap_or(""))
                .is_some()
            {
                e["scope"] = json!("local");
                e["editable"] = json!(false);
                e["removable"] = json!(false);
                e["note"] = json!("사용자 파일의 프로젝트 로컬 설정이 공유 설정보다 우선합니다.");
            }
        }
    }
    Ok(
        json!({"entries":entries,"errors":errors,"notes":notes,"revision":digest(root,provider)?,"workspace":root,"host_id":"local","can_install_plugin":*provider==Provider::Claude,"supported":true,"apply":"next_turn"}),
    )
}
pub async fn list(engine: &Engine, project_id: &str, provider_id: &str) -> Result<Value> {
    let provider = engine.store.provider(provider_id)?;
    if provider.host_id != "local" {
        return crate::peer::project_command(
            engine,
            &provider,
            project_id,
            json!({"type":"list_project_extensions"}),
        )
        .await;
    }
    let project = engine.store.project(project_id)?;
    if !matches!(provider.adapter, Provider::Codex | Provider::Claude) {
        return Ok(
            json!({"supported":false,"entries":[],"errors":["이 제공자는 프로젝트 스킬·플러그인·MCP 관리 인터페이스를 제공하지 않습니다."],"notes":[],"workspace":project.workspace,"host_id":"local"}),
        );
    }
    let root = Path::new(&project.workspace)
        .canonicalize()
        .context("실행 호스트에 프로젝트 폴더가 없습니다.")?;
    let _guard = lock().lock().await;
    local_view(&engine.configured(provider_id)?, &root, &provider.adapter).await
}
pub async fn update(
    engine: &Engine,
    project_id: &str,
    provider_id: &str,
    edit: Edit,
) -> Result<Value> {
    let provider = engine.store.provider(provider_id)?;
    if provider.host_id != "local" {
        return crate::peer::project_command(
            engine,
            &provider,
            project_id,
            json!({"type":"update_project_extension","edit":edit}),
        )
        .await;
    }
    if !matches!(provider.adapter, Provider::Codex | Provider::Claude) {
        bail!("프로젝트 확장 관리 미지원 제공자입니다.");
    }
    let configured = engine.configured(provider_id)?;
    let project = engine.store.project(project_id)?;
    let root = Path::new(&project.workspace).canonicalize()?;
    let _guard = lock().lock().await;
    if digest(&root, &provider.adapter)? != edit.revision {
        bail!("다른 곳에서 설정이 변경되었습니다. 새로고침 후 다시 저장하세요.");
    }
    let view = local_view(&configured, &root, &provider.adapter).await?;
    let entry = view["entries"].as_array().unwrap().iter().find(|e| {
        e["kind"] == edit.kind
            && (e["id"] == edit.id
                || edit.kind == "skill"
                    && e["id"]
                        .as_str()
                        .is_some_and(|id| same_skill_path(id, &edit.id)))
    });
    if edit.action != "add" && entry.is_none() {
        bail!("선택한 확장이 더 이상 없습니다.");
    }
    if edit.action == "add" && entry.is_some() {
        bail!("같은 이름의 확장이 이미 있습니다. 해당 항목을 편집하세요.");
    }
    let path = config_path(&root, &provider.adapter)?;
    let expected = revision(&text(&path)?);
    match edit.kind.as_str() {
        "skill" => {
            if edit.action == "toggle" {
                let enabled = edit.value.as_bool().context("사용 여부를 선택하세요.")?;
                if provider.adapter == Provider::Codex {
                    let (mut doc, local) = toml_file(&path)?;
                    let mut skills = local["skills"]["config"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    let skill_path = entry.unwrap()["id"].as_str().context("스킬 경로 없음")?;
                    skills.retain(|s| {
                        !s["path"]
                            .as_str()
                            .is_some_and(|path| same_skill_path(path, skill_path))
                    });
                    skills.push(json!({"path":skill_path,"enabled":enabled}));
                    doc["skills"]["config"] = item(&json!(skills))?;
                    write(&path, &expected, &doc.to_string())?;
                } else {
                    let mut settings = json_file(&path)?;
                    let name = entry.unwrap()["name"].as_str().context("스킬 이름 없음")?;
                    object(&mut settings, "skillOverrides")?
                        .insert(name.into(), json!(if enabled { "on" } else { "off" }));
                    write_json(&path, &expected, &settings)?;
                }
            } else {
                let target = if edit.action == "add" {
                    safe_name(&edit.id)?;
                    if edit.id.contains([':', '@', '.']) {
                        bail!("스킬 폴더 이름에는 글자·숫자·밑줄·하이픈만 사용하세요.");
                    }
                    skill_root(&root, &provider.adapter)
                        .join(&edit.id)
                        .join("SKILL.md")
                } else {
                    if entry.unwrap()["editable"] != true {
                        bail!("상속된 스킬은 원본 위치에서 편집하세요.");
                    }
                    PathBuf::from(&edit.id)
                };
                let target = scoped(
                    &root,
                    project_relative(&root, &target)
                        .context("프로젝트 스킬만 편집할 수 있습니다.")?,
                )?;
                let old = revision(&text(&target)?);
                match edit.action.as_str() {
                    "add" | "save" => {
                        let content = edit.value.as_str().context("SKILL.md 내용을 입력하세요.")?;
                        if content.trim().is_empty() {
                            bail!("빈 스킬을 저장할 수 없습니다.");
                        }
                        write(&target, &old, content)?;
                    }
                    "remove" => {
                        let backup = target.with_file_name(format!(
                            "SKILL.md.bibi-removed-{}",
                            uuid::Uuid::new_v4()
                        ));
                        std::fs::rename(target, backup)?;
                    }
                    _ => bail!("지원하지 않는 스킬 조작입니다."),
                }
            }
        }
        "plugin" => {
            safe_name(&edit.id)?;
            match edit.action.as_str() {
                "add" => {
                    if provider.adapter != Provider::Claude {
                        bail!(
                            "Codex 플러그인 설치는 사용자 전체에 적용되므로 원본 앱에서 설치하세요."
                        );
                    }
                    claude_cli(
                        &configured,
                        &root,
                        &[
                            "plugin", "install", &edit.id, "--scope", "project", "--json",
                        ],
                    )
                    .await?;
                }
                "toggle" => {
                    if entry.unwrap()["toggleable"] != true {
                        bail!("관리 정책으로 변경할 수 없는 플러그인입니다.");
                    }
                    let enabled = edit.value.as_bool().context("사용 여부를 선택하세요.")?;
                    if provider.adapter == Provider::Codex {
                        let (mut doc, _) = toml_file(&path)?;
                        doc["plugins"][&edit.id]["enabled"] = toml_edit::value(enabled);
                        write(&path, &expected, &doc.to_string())?;
                    } else {
                        let mut settings = json_file(&path)?;
                        object(&mut settings, "enabledPlugins")?
                            .insert(edit.id.clone(), json!(enabled));
                        write_json(&path, &expected, &settings)?;
                    }
                }
                "remove" => {
                    if entry.unwrap()["removable"] != true {
                        bail!("이 프로젝트에 설치된 플러그인만 제거할 수 있습니다.");
                    }
                    if provider.adapter == Provider::Codex {
                        let (mut doc, _) = toml_file(&path)?;
                        if let Some(map) = doc["plugins"].as_table_like_mut() {
                            map.remove(&edit.id);
                        }
                        write(&path, &expected, &doc.to_string())?;
                    } else {
                        claude_cli(
                            &configured,
                            &root,
                            &[
                                "plugin",
                                "uninstall",
                                &edit.id,
                                "--scope",
                                "project",
                                "--json",
                            ],
                        )
                        .await?;
                    }
                }
                _ => bail!("지원하지 않는 플러그인 조작입니다."),
            }
        }
        "mcp" => {
            safe_name(&edit.id)?;
            if edit.id == "bibi" {
                bail!("BiBi 내부 MCP 이름은 사용할 수 없습니다.");
            }
            let path = mcp_path(&root, &provider.adapter)?;
            let expected = revision(&text(&path)?);
            if edit.action == "toggle" && provider.adapter == Provider::Claude {
                let enabled = edit.value.as_bool().context("사용 여부를 선택하세요.")?;
                toggle_claude_mcp(
                    &root,
                    &claude_account()?,
                    &edit.id,
                    enabled,
                    entry.unwrap()["scope"] == "project",
                )?;
            } else {
                let (mut doc, mut data) = if provider.adapter == Provider::Codex {
                    let (d, v) = toml_file(&path)?;
                    (Some(d), v)
                } else {
                    (None, json_file(&path)?)
                };
                let key = if provider.adapter == Provider::Codex {
                    "mcp_servers"
                } else {
                    "mcpServers"
                };
                let current = data[key][&edit.id].clone();
                if edit.action != "add"
                    && edit.action != "toggle"
                    && entry.unwrap()["editable"] != true
                {
                    bail!("상속된 MCP 설정은 원본 위치에서 편집하세요.");
                }
                match edit.action.as_str() {
                    "add" | "save" => {
                        let mut config = edit.value.clone();
                        restore(&mut config, &current)?;
                        probe::validate(&config)?;
                        object(&mut data, key)?.insert(edit.id.clone(), config);
                    }
                    "toggle" => {
                        let enabled = edit.value.as_bool().context("사용 여부를 선택하세요.")?;
                        let map = object(&mut data, key)?;
                        let config = map.entry(edit.id.clone()).or_insert(json!({}));
                        config["enabled"] = json!(enabled);
                    }
                    "remove" => {
                        object(&mut data, key)?.remove(&edit.id);
                    }
                    _ => bail!("지원하지 않는 MCP 조작입니다."),
                }
                if let Some(ref mut doc) = doc {
                    if edit.action == "remove" {
                        if let Some(table) = doc[key].as_table_like_mut() {
                            table.remove(&edit.id);
                        }
                    } else if edit.action == "toggle" {
                        doc[key][&edit.id]["enabled"] =
                            toml_edit::value(edit.value.as_bool().unwrap());
                    } else {
                        doc[key][&edit.id] = item(&data[key][&edit.id])?;
                    }
                    write(&path, &expected, &doc.to_string())?;
                } else {
                    write_json(&path, &expected, &data)?;
                }
            }
        }
        _ => bail!("지원하지 않는 확장 종류입니다."),
    }
    engine.invalidate_project(&project.id).await?;
    Ok(json!({"saved":true,"apply":"next_turn"}))
}
fn toggle_claude_mcp(
    root: &Path,
    account_path: &Path,
    id: &str,
    enabled: bool,
    project_entry: bool,
) -> Result<()> {
    if std::fs::symlink_metadata(account_path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("사용자 설정이 심볼릭 링크입니다. 원본 CLI에서 사용 여부를 변경하세요.");
    }
    let account_old = text(account_path)?;
    let mut account = json_file(account_path)?;
    let projects = object(&mut account, "projects")?;
    let project = projects
        .entry(root.to_string_lossy().into_owned())
        .or_insert(json!({}));
    let mut disabled = project["disabledMcpServers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    disabled.retain(|s| s != &json!(id));
    if !enabled {
        disabled.push(json!(id));
    }
    project["disabledMcpServers"] = json!(disabled);
    let settings_path = config_path(root, &Provider::Claude)?;
    let settings_old = text(&settings_path)?;
    let mut settings = json_file(&settings_path)?;
    if enabled && project_entry {
        for field in ["disabledMcpjsonServers", "enabledMcpjsonServers"] {
            let mut values = settings[field].as_array().cloned().unwrap_or_default();
            values.retain(|s| s != &json!(id));
            if field == "enabledMcpjsonServers" {
                values.push(json!(id));
            }
            settings[field] = json!(values);
        }
    }
    write_json(account_path, &revision(&account_old), &account)?;
    if enabled
        && project_entry
        && let Err(error) = write_json(&settings_path, &revision(&settings_old), &settings)
    {
        let saved = serde_json::to_string_pretty(&account)? + "\n";
        if write(account_path, &revision(&saved), &account_old).is_err() {
            bail!("설정 저장 중 외부 변경이 발생했습니다. 두 파일의 현재 상태를 새로고침하세요.");
        }
        return Err(error);
    }
    Ok(())
}

pub async fn check_mcp(
    engine: &Engine,
    project_id: &str,
    provider_id: &str,
    id: &str,
) -> Result<Value> {
    let provider = engine.store.provider(provider_id)?;
    if provider.host_id != "local" {
        return crate::peer::project_command(
            engine,
            &provider,
            project_id,
            json!({"type":"check_project_mcp","id":id}),
        )
        .await;
    }
    safe_name(id)?;
    if id == "bibi" {
        bail!("내부 MCP는 실행 중인 세션에서 연결됩니다.");
    }
    if !matches!(provider.adapter, Provider::Codex | Provider::Claude) {
        bail!("MCP 조회 미지원 제공자입니다.");
    }
    let configured = engine.configured(provider_id)?;
    let project = engine.store.project(project_id)?;
    let root = Path::new(&project.workspace).canonicalize()?;
    let config = if provider.adapter == Provider::Codex {
        let mut rpc =
            super::rpc::Rpc::connect_project(&configured.config, &root.to_string_lossy()).await?;
        rpc.request("config/read", json!({"cwd":root,"includeLayers":false}))
            .await?["config"]["mcp_servers"][id]
            .clone()
    } else {
        let account = json_file(&claude_account()?)?;
        let project_key = root.to_string_lossy();
        let local = json_file(&mcp_path(&root, &provider.adapter)?)?;
        let candidates = [
            &account["projects"][project_key.as_ref()]["mcpServers"][id],
            &local["mcpServers"][id],
            &account["mcpServers"][id],
        ];
        candidates
            .into_iter()
            .find(|v| v.is_object())
            .cloned()
            .context("MCP 설정이 없습니다.")?
    };
    probe::check(&config, &root).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn windows_skill_paths_match_native_and_canonical_forms_without_crossing_projects() {
        let root = Path::new(r"\\?\C:\workspace\one");
        let native = r"C:\workspace\one\.agents\skills\local\SKILL.md";
        let canonical = r"\\?\C:\workspace\one\.agents\skills\local\SKILL.md";
        assert!(same_skill_path(native, canonical));
        assert_eq!(
            project_relative(root, Path::new(native)),
            Some(Path::new(r".agents\skills\local\SKILL.md"))
        );
        assert_eq!(scope(root, Path::new(native)), "project");
        assert_eq!(
            scope(root, Path::new(r"C:\workspace\one-other\SKILL.md")),
            "inherited"
        );
        assert!(!same_skill_path(
            native,
            r"D:\workspace\one\.agents\skills\local\SKILL.md"
        ));
    }

    #[test]
    fn claude_mcp_toggles_preserve_credentials_other_projects_and_malformed_settings() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let account = temp.path().join("account.json");
        let original = json!({"credential":"preserve-secret","projects":{"other":{"disabledMcpServers":["other"]}}});
        write_json(&account, &revision(""), &original).unwrap();
        toggle_claude_mcp(&root, &account, "test", false, true).unwrap();
        let disabled = json_file(&account).unwrap();
        assert_eq!(disabled["credential"], original["credential"]);
        assert_eq!(disabled["projects"]["other"], original["projects"]["other"]);
        assert_eq!(
            disabled["projects"][root.to_string_lossy().as_ref()]["disabledMcpServers"],
            json!(["test"])
        );
        toggle_claude_mcp(&root, &account, "test", true, true).unwrap();
        assert_eq!(
            json_file(&root.join(".claude/settings.local.json")).unwrap()["enabledMcpjsonServers"],
            json!(["test"])
        );
        let before = text(&account).unwrap();
        std::fs::write(root.join(".claude/settings.local.json"), "broken").unwrap();
        assert!(toggle_claude_mcp(&root, &account, "another", false, true).is_err());
        assert_eq!(text(&account).unwrap(), before);
    }
}
