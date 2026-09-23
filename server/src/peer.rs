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
}
async fn api<T: serde::de::DeserializeOwned>(
    peer: &PeerConfig,
    path: &str,
    body: Option<Value>,
) -> Result<T> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
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
    engine.store.upsert_host(host.clone())?;
    engine
        .store
        .replace_remote_providers(&host.id, snapshot.providers)?;
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
            for run in snapshot.runs.iter().filter(|r| r.host_id == "local") {
                if engine
                    .store
                    .setting::<String>(&format!("host_workspace:{}:{}", host.id, run.project_key))?
                    .is_none()
                {
                    continue;
                }
                let Some(work) = snapshot.works.iter().find(|w| w.id == run.work_id) else {
                    continue;
                };
                let adopted = engine
                    .store
                    .adopt_remote(&host.id, run.clone(), work.clone())?;
                let old = engine.store.run(&run.id)?;
                if (adopted || old.updated_at < run.updated_at || run.origin == Origin::External)
                    && let Ok(remote) =
                        api::<HostRun>(&peer, &format!("/api/host/runs/{}", run.id), None).await
                    && remote.server_id == peer.id
                {
                    engine
                        .store
                        .mirror_remote(&host.id, remote.detail, remote.transmissions)?;
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
        let job = ForwardJob {
            run: remote_run,
            project,
            work: engine.store.work(&run.work_id)?,
        };
        engine.store.set_setting(&key, &job)?;
        job
    };
    let body = serde_json::to_value(&job)?;
    let mut accepted = engine
        .store
        .setting::<bool>(&format!("remote_owned:{}", run.id))?
        .unwrap_or(false);
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
                let mirrored=engine.store.mirror_remote(&run.host_id,remote.detail,remote.transmissions)?;
                if mirrored.state.terminal(){return Ok(());}
                for input in engine.store.inputs(&run.id)?.into_iter().filter(|i|i.state=="accepted"||i.state=="sending") {
                    if input.state=="accepted" {engine.store.input_state(&input.id,"sending")?;}
                    let request=Submission{submission_id:input.id.clone(),project_key:run.project_key.clone(),work_id:Some(run.work_id.clone()),title:None,
                        question:input.text,provider:run.provider.clone(),provider_id:job.run.provider_id.clone(),model:run.model.clone(),host_id:"local".into(),role:run.role.clone(),mode:SubmitMode::Steer,
                        target_run_id:Some(run.id.clone()),expected_turn_id:Some(input.expected_turn_id),expected_context_revision:Some(run.context_revision),read_only:run.read_only};
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
#[derive(Deserialize)]
pub struct Validate {
    workspace: String,
    guild_path: Option<String>,
}
fn directory(value: &str) -> Result<String, ApiError> {
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
        detail,
        transmissions,
    }))
}
