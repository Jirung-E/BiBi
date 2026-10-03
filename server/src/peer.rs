use crate::{
    ApiError, AppState, Command,
    runtime::{Control, Engine},
};
use anyhow::{Context, Result, bail};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use bibi_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub token: String,
}
#[derive(Serialize, Deserialize)]
pub struct HostRun {
    server_id: String,
    detail: RunDetail,
    transmissions: Vec<Transmission>,
    #[serde(default)]
    history: Vec<Run>,
}
async fn api<T: serde::de::DeserializeOwned>(
    peer: &PeerConfig,
    path: &str,
    body: Option<Value>,
) -> Result<T> {
    api_timeout(peer, path, body, Duration::from_secs(10)).await
}
async fn api_timeout<T: serde::de::DeserializeOwned>(
    peer: &PeerConfig,
    path: &str,
    body: Option<Value>,
    timeout: Duration,
) -> Result<T> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let request = if let Some(body) = body {
        client.post(format!("{}{path}", peer.url)).json(&body)
    } else {
        client.get(format!("{}{path}", peer.url))
    };
    let response = request.bearer_auth(&peer.token).send().await?;
    let status = response.status();
    let value: Value = response.json().await?;
    if !status.is_success() {
        bail!(
            "원격 호스트 {}: {}",
            status,
            value["error"].as_str().unwrap_or("응답 오류")
        );
    }
    Ok(serde_json::from_value(value)?)
}
pub async fn register(
    engine: &Engine,
    name: String,
    url: String,
    token: String,
    project_key: String,
    workspace: String,
    guild_path: Option<String>,
) -> Result<Host> {
    let parsed = reqwest::Url::parse(&url)?;
    if !["http", "https"].contains(&parsed.scheme())
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("HTTP(S) 호스트 주소가 필요합니다.");
    }
    let _: Project = engine.store.project(&project_key)?;
    if name.trim().is_empty() || token.trim().len() < 32 {
        bail!("호스트 이름과 인증 토큰이 필요합니다.");
    }
    let mut peer = PeerConfig {
        id: String::new(),
        name: name.clone(),
        url: url.trim_end_matches('/').into(),
        token: token.trim().into(),
    };
    let snapshot: Snapshot = api(&peer, "/api/snapshot", None).await?;
    if snapshot.server_id == engine.store.server_id()? {
        bail!("자기 자신을 원격 호스트로 등록할 수 없습니다.");
    }
    peer.id = snapshot.server_id;
    let validated: Value = api(
        &peer,
        "/api/host/validate",
        Some(json!({"workspace":workspace,"guild_path":guild_path})),
    )
    .await?;
    let workspace = validated["workspace"]
        .as_str()
        .context("호스트 작업 경로 응답 오류")?;
    let candidates = snapshot
        .projects
        .iter()
        .filter(|p| p.workspace == workspace)
        .collect::<Vec<_>>();
    let remote_project = if let Some(p) = candidates.iter().find(|p| p.id == project_key) {
        p.id.clone()
    } else if candidates.len() == 1 {
        candidates[0].id.clone()
    } else if candidates.is_empty() {
        project_key.clone()
    } else {
        bail!(
            "이 원격 경로에 프로젝트가 여러 개 있습니다. 사용할 프로젝트의 작업 경로를 구분하세요."
        );
    };
    for local_project in engine.store.snapshot()?.projects {
        if local_project.id != project_key
            && engine
                .store
                .setting::<String>(&format!("host_project:{}:{}", peer.id, local_project.id))?
                .as_deref()
                == Some(&remote_project)
        {
            bail!("이 원격 프로젝트는 이미 다른 로컬 프로젝트에 연결되어 있습니다.");
        }
    }
    let local = snapshot
        .hosts
        .into_iter()
        .find(|h| h.id == "local")
        .context("호스트 실행 서비스가 시작되지 않았습니다.")?;
    let host = Host {
        id: peer.id.clone(),
        name,
        platform: local.platform,
        kind: "peer".into(),
        connected: true,
        observed_at: now(),
        providers: local.providers,
        error: None,
    };
    engine
        .store
        .set_setting(&format!("peer:{}", peer.id), &peer)?;
    engine.store.set_setting(
        &format!("host_workspace:{}:{}", peer.id, project_key),
        &workspace,
    )?;
    engine.store.set_setting(
        &format!("host_guild:{}:{}", peer.id, project_key),
        &validated["guild_path"],
    )?;
    engine.store.set_setting(
        &format!("host_project:{}:{}", peer.id, project_key),
        &remote_project,
    )?;
    engine.store.upsert_host(host.clone())?;
    engine
        .store
        .replace_remote_providers(&host.id, snapshot.providers)?;
    refresh(engine, host.clone()).await?;
    Ok(host)
}
pub fn config(engine: &Engine, host_id: &str) -> Result<PeerConfig> {
    engine
        .store
        .setting(&format!("peer:{host_id}"))?
        .context("원격 호스트 연결 정보가 없습니다.")
}
pub async fn refresh(engine: &Engine, mut host: Host) -> Result<()> {
    let peer = config(engine, &host.id)?;
    match api::<Snapshot>(&peer, "/api/snapshot", None).await {
        Ok(snapshot) if snapshot.server_id == peer.id => {
            engine
                .store
                .replace_remote_providers(&host.id, snapshot.providers.clone())?;
            host.connected = true;
            host.observed_at = now();
            host.error = None;
            let local_projects = engine.store.snapshot()?.projects;
            for run in snapshot.runs.iter().filter(|r| r.host_id == "local") {
                let mut local_project = None;
                for project in &local_projects {
                    if engine
                        .store
                        .setting::<String>(&format!("host_workspace:{}:{}", host.id, project.id))?
                        .is_some()
                        && remote_project(engine, &host.id, &project.id)? == run.project_key
                    {
                        local_project = Some(project.id.clone());
                        break;
                    }
                }
                let Some(local_project) = local_project else {
                    continue;
                };
                let Some(work) = snapshot.works.iter().find(|w| w.id == run.work_id) else {
                    continue;
                };
                let mut adopted_run = run.clone();
                project_run(&mut adopted_run, &local_project);
                let mut work = work.clone();
                work.project_key = local_project.clone();
                let adopted = engine.store.adopt_remote(&host.id, adopted_run, work)?;
                let old = engine.store.run(&run.id)?;
                if (adopted
                    || old.updated_at < run.updated_at
                    || old.state != run.state
                    || !run.state.terminal()
                    || run.origin == Origin::External)
                    && let Ok(remote) =
                        api::<HostRun>(&peer, &format!("/api/host/runs/{}", run.id), None).await
                {
                    mirror(engine, &peer, &local_project, remote)?;
                }
            }
            for provider in [
                Provider::Codex,
                Provider::Claude,
                Provider::Ollama,
                Provider::OpenAi,
                Provider::Command,
                Provider::Mock,
            ] {
                let quotas = snapshot
                    .quotas
                    .iter()
                    .filter(|q| q.host_id == "local" && q.provider == provider)
                    .cloned()
                    .map(|mut quota| {
                        quota.id = format!("{}:{}", host.id, quota.id);
                        quota.host_id = host.id.clone();
                        quota.provider_id = quota
                            .provider_id
                            .map(|id| remote_provider_id(&host.id, &id));
                        quota
                    })
                    .collect();
                engine.store.replace_quotas(&provider, &host.id, quotas)?;
            }
        }
        Ok(_) => {
            host.connected = false;
            host.error = Some("원격 서버 ID가 변경되었습니다. 다시 연결하세요.".into());
        }
        Err(error) => {
            host.connected = false;
            host.error = Some(error.to_string());
        }
    }
    engine.store.upsert_host(host)?;
    Ok(())
}
fn disconnected(engine: &Engine, run: &Run, error: &anyhow::Error) {
    let current = engine.store.run(&run.id).ok();
    if current.is_some_and(|r| r.state != RunState::Disconnected) {
        let _ = engine.store.observe(
            &run.id,
            RunState::Disconnected,
            "호스트 연결 끊김",
            Some(error.to_string()),
        );
    }
}
pub async fn execute(
    engine: &Engine,
    run: Run,
    mut controls: mpsc::Receiver<Control>,
) -> Result<()> {
    let peer = config(engine, &run.host_id)?;
    // Freeze the original transfer, including context and paths, before any network request.
    let key = format!("forward_job:{}", run.id);
    let job = if let Some(job) = engine.store.setting::<ForwardJob>(&key)? {
        job
    } else {
        let mut project = engine.store.project(&run.project_key)?;
        project.workspace = run.workspace.clone();
        project.guild_path = engine
            .store
            .setting::<Option<String>>(&format!("host_guild:{}:{}", run.host_id, run.project_key))?
            .flatten();
        let mut remote_run = run.clone();
        if let Some(id) = &run.provider_id {
            remote_run.provider_id = engine.store.provider(id)?.remote_id;
        }
        let remote_project = remote_project(engine, &run.host_id, &run.project_key)?;
        project.id = remote_project.clone();
        project_run(&mut remote_run, &remote_project);
        let mut work = engine.store.work(&run.work_id)?;
        work.project_key = remote_project;
        let job = ForwardJob {
            run: remote_run,
            project,
            work,
        };
        engine.store.set_setting(&key, &job)?;
        job
    };
    let mut accepted = engine
        .store
        .setting::<bool>(&format!("remote_owned:{}", run.id))?
        .unwrap_or(false);
    let prior_mode = run
        .continued_from
        .as_deref()
        .map(|id| engine.store.run(id))
        .transpose()?
        .map(|r| r.approval_mode)
        .unwrap_or_default();
    if !accepted
        && (run.approval_mode != ApprovalMode::OnRequest || prior_mode != ApprovalMode::OnRequest)
    {
        let capabilities: Value = api(&peer, "/api/host/capabilities", None).await
            .context("원격 BiBi의 승인 모드 지원을 확인하지 못해 이번 전송을 중단했습니다. 원격 연결과 앱 업데이트 여부를 확인하세요.")?;
        if capabilities["server_id"].as_str() != Some(&peer.id)
            || capabilities["approval_modes_v1"] != true
        {
            bail!(
                "원격 BiBi가 승인 모드 변경을 지원하지 않아 이번 전송을 중단했습니다. 원격 앱을 업데이트하세요."
            );
        }
    }
    let body = serde_json::to_value(&job)?;
    let mut interval = tokio::time::interval(Duration::from_millis(400));
    loop {
        tokio::select! {
            _=interval.tick()=>{
                if engine.is_stopping(){return Ok(());}
                if !accepted {
                    match api::<Run>(&peer,"/api/host/execute",Some(body.clone())).await {
                        Ok(_)=>accepted=true,
                        Err(error)=>{disconnected(engine,&run,&error);continue;}
                    }
                }
                let remote=match api::<HostRun>(&peer,&format!("/api/host/runs/{}",run.id),None).await {
                    Ok(remote)=>remote,Err(error)=>{disconnected(engine,&run,&error);continue;}
                };
                if remote.server_id!=peer.id {disconnected(engine,&run,&anyhow::anyhow!("호스트 ID가 변경되었습니다."));continue;}
                let mirrored=mirror(engine,&peer,&run.project_key,remote)?;
                if mirrored.state.terminal(){return Ok(());}
                for input in engine.store.inputs(&run.id)?.into_iter().filter(|i|i.state=="accepted"||i.state=="sending") {
                    if input.state=="accepted" {engine.store.input_state(&input.id,"sending")?;}
                    let request=Submission{submission_id:input.id.clone(),project_key:job.run.project_key.clone(),work_id:Some(run.work_id.clone()),title:None,
                        question:input.text,provider:run.provider.clone(),provider_id:job.run.provider_id.clone(),model:run.model.clone(),host_id:"local".into(),role:run.role.clone(),mode:SubmitMode::Steer,
                        target_run_id:Some(run.id.clone()),expected_turn_id:Some(input.expected_turn_id),expected_context_revision:Some(run.context_revision),read_only:run.read_only,approval_mode:None};
                    // Safe retries share the durable remote submission key. Runtime delivery is mirrored separately.
                    if let Err(error)=api::<Value>(&peer,"/api/command",Some(serde_json::to_value(Command::Submit{request})?)).await {
                        disconnected(engine,&run,&error);
                    }
                }
            },
            control=controls.recv()=>match control {
                Some(Control::Interrupt)=>{
                    if let Err(error)=api::<Value>(&peer,"/api/command",Some(json!({"type":"interrupt","run_id":run.id}))).await {
                        disconnected(engine,&run,&error);
                        engine.store.add_message(&run.id,"system","원격 중단을 확인하지 못했습니다. 연결 복구 후 상태를 확인하세요.")?;
                    }
                },
                Some(Control::Respond{approval_id,value,reply})=>{
                    let response=api::<Value>(&peer,"/api/command",Some(json!({"type":"respond","approval_id":approval_id,"value":value}))).await.map(|_|());
                    let _=reply.send(response);
                },
                None=>return Ok(()),
            }
        }
    }
}
pub async fn capabilities(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({"server_id":s.store.server_id()?,"approval_modes_v1":true,"project_extensions_v1":true}),
    ))
}

#[derive(Deserialize)]
pub struct Validate {
    workspace: String,
    guild_path: Option<String>,
}
pub(crate) fn directory(value: &str) -> Result<String, ApiError> {
    let path = std::path::PathBuf::from(value)
        .canonicalize()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "호스트 경로가 존재하지 않습니다."))?;
    if !path.is_dir() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "경로는 폴더여야 합니다.",
        ));
    }
    Ok(path.to_string_lossy().into())
}
pub async fn validate(Json(v): Json<Validate>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({"workspace":directory(&v.workspace)?,"guild_path":v.guild_path.as_deref().map(directory).transpose()?}),
    ))
}
pub async fn accept(
    State(s): State<AppState>,
    Json(mut job): Json<ForwardJob>,
) -> Result<Json<Run>, ApiError> {
    if let Some(id) = &job.run.provider_id {
        let provider = s.store.provider(id)?;
        if provider.host_id != "local" || provider.adapter != job.run.provider {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "원격 제공자 설정이 일치하지 않습니다.",
            ));
        }
    } else if job.run.provider != Provider::Mock {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "이 호스트에 제공자를 등록하세요.",
        ));
    }
    job.run.workspace = directory(&job.run.workspace)?;
    job.run.context.workspace = job.run.workspace.clone();
    job.project.workspace = job.run.workspace.clone();
    job.project.guild_path = job
        .project
        .guild_path
        .as_deref()
        .map(directory)
        .transpose()?;
    Ok(Json(s.store.accept_forwarded(job)?))
}
pub async fn detail(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<HostRun>, ApiError> {
    let detail = s.store.detail(&id)?;
    let mut transmissions = s.store.request_transmissions(&detail.run.request_id)?;
    for child in &detail.children {
        transmissions.extend(s.store.request_transmissions(&child.run.request_id)?);
    }
    transmissions.sort_by_key(|t| t.sent_at);
    transmissions.dedup_by(|a, b| a.id == b.id);
    Ok(Json(HostRun {
        server_id: s.store.server_id()?,
        history: s
            .store
            .work_runs(&detail.run.work_id)?
            .into_iter()
            .filter(|r| r.host_id == "local")
            .collect(),
        detail,
        transmissions,
    }))
}

fn remote_project(engine: &Engine, host: &str, local: &str) -> Result<String> {
    Ok(engine
        .store
        .setting::<String>(&format!("host_project:{host}:{local}"))?
        .unwrap_or_else(|| local.into()))
}
fn project_run(run: &mut Run, project: &str) {
    run.project_key = project.into();
    run.context.project_key = project.into();
}
fn mirror(engine: &Engine, peer: &PeerConfig, project: &str, mut remote: HostRun) -> Result<Run> {
    if remote.server_id != peer.id {
        bail!("원격 서버 ID가 변경되었습니다.");
    }
    let expected = remote_project(engine, &peer.id, project)?;
    for run in std::iter::once(&mut remote.detail.run)
        .chain(remote.detail.children.iter_mut().map(|c| &mut c.run))
        .chain(&mut remote.history)
    {
        if run.project_key != expected || run.context.project_key != expected {
            bail!("원격 프로젝트가 변경되었습니다.");
        }
        project_run(run, project);
    }
    Ok(engine.store.mirror_remote_history(
        &peer.id,
        remote.detail,
        remote.transmissions,
        remote.history,
    )?)
}

pub async fn project_command(
    engine: &Engine,
    provider: &ProviderConfig,
    project: &str,
    mut body: Value,
) -> Result<Value> {
    let peer = config(engine, &provider.host_id)?;
    let caps: Value = api(&peer, "/api/host/capabilities", None).await?;
    if caps["server_id"].as_str() != Some(&peer.id) || caps["project_extensions_v1"] != true {
        bail!(
            "이 원격 BiBi 서버는 프로젝트 명령·확장 관리를 지원하지 않습니다. 원격 서버를 업데이트하세요."
        );
    }
    let mut target = engine.store.project(project)?;
    target.id = remote_project(engine, &provider.host_id, project)?;
    target.workspace = engine
        .store
        .setting::<String>(&format!("host_workspace:{}:{}", provider.host_id, project))?
        .context("원격 프로젝트 작업 경로가 없습니다.")?;
    target.guild_path = engine
        .store
        .setting::<Option<String>>(&format!("host_guild:{}:{}", provider.host_id, project))?
        .flatten();
    let _: Project = api(
        &peer,
        "/api/command",
        Some(json!({"type":"prepare_host_project","project":target})),
    )
    .await?;
    body["project_key"] = json!(target.id);
    body["provider_id"] = json!(
        provider
            .remote_id
            .as_ref()
            .context("원격 제공자 식별자 없음")?
    );
    api_timeout(&peer, "/api/command", Some(body), Duration::from_secs(120)).await
}
