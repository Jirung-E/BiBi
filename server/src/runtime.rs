use crate::config::ServiceConfig;
use anyhow::{Result, bail};
use bibi_core::*;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, mpsc, oneshot};

pub enum Control {
    Interrupt,
    Respond {
        approval_id: String,
        value: Value,
        reply: oneshot::Sender<Result<()>>,
    },
}
#[derive(Clone)]
pub struct Engine {
    pub store: Store,
    pub config: ServiceConfig,
    pub provider: Option<ProviderConfig>,
    api_key: Option<String>,
    active: Arc<Mutex<HashMap<String, mpsc::Sender<Control>>>>,
    stopping: Arc<AtomicBool>,
    pub(crate) codex_sessions:
        Arc<crate::providers::sessions::Sessions<crate::providers::rpc::Rpc>>,
    pub(crate) claude_sessions:
        Arc<crate::providers::sessions::Sessions<crate::providers::claude::Connection>>,
}
impl Engine {
    pub fn new(store: Store, config: ServiceConfig) -> Self {
        Self {
            store,
            config,
            provider: None,
            api_key: None,
            active: Arc::new(Mutex::new(HashMap::new())),
            stopping: Arc::new(AtomicBool::new(false)),
            codex_sessions: Arc::default(),
            claude_sessions: Arc::default(),
        }
    }
    pub fn configured(&self, id: &str) -> Result<Self> {
        let provider = self.store.provider(id)?;
        if provider.host_id != "local" {
            bail!("원격 제공자는 해당 호스트에서 실행해야 합니다.");
        }
        let mut engine = self.clone();
        match provider.adapter {
            Provider::Codex => {
                engine.config.codex_command = provider.command.clone();
                engine.config.codex_args = provider.args.clone();
            }
            Provider::Claude => {
                engine.config.claude_command = provider.command.clone();
                engine.config.claude_args = provider.args.clone();
            }
            Provider::Ollama => engine.config.ollama_url = provider.endpoint.clone(),
            _ => (),
        }
        engine.api_key = self.store.provider_secret(id)?;
        engine.provider = Some(provider);
        Ok(engine)
    }
    pub fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }
    pub fn provider_id(&self) -> Option<String> {
        self.provider.as_ref().map(|p| p.id.clone())
    }
    pub fn quota_id(&self, fallback: &str) -> String {
        format!(
            "local:{}",
            self.provider
                .as_ref()
                .map(|p| p.id.as_str())
                .unwrap_or(fallback)
        )
    }
    pub fn replace_quotas(&self, adapter: &Provider, quotas: Vec<Quota>) -> Result<()> {
        if let Some(id) = self.provider_id() {
            self.store.replace_connection_quotas(&id, adapter, quotas)?;
        } else {
            self.store.replace_quotas(adapter, "local", quotas)?;
        }
        Ok(())
    }
    pub async fn discard_session(&self, session: &str) {
        self.codex_sessions.take(session).await;
        self.claude_sessions.take(session).await;
    }
    pub async fn discard_provider(&self, provider_id: &str) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        for run in snapshot
            .runs
            .iter()
            .chain(&snapshot.removed_sessions)
            .filter(|r| r.provider_id.as_deref() == Some(provider_id))
        {
            self.discard_session(run.session_id()).await;
        }
        Ok(())
    }
    pub async fn start(&self) -> Result<()> {
        self.store.migrate_sessions()?;
        self.store.recover_host("local")?;
        self.store.upsert_host(Host {
            id: "local".into(),
            name: std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "이 호스트".into()),
            platform: std::env::consts::OS.into(),
            kind: "local".into(),
            connected: true,
            observed_at: now(),
            providers: vec![
                Provider::Mock,
                Provider::Codex,
                Provider::Claude,
                Provider::Ollama,
                Provider::OpenAi,
                Provider::Command,
            ],
            error: None,
        })?;
        let engine = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(200));
            let mut heartbeat = tokio::time::Instant::now();
            loop {
                interval.tick().await;
                if engine.stopping.load(Ordering::Relaxed) {
                    break;
                }
                let hosts = match engine.store.hosts() {
                    Ok(hosts) => hosts,
                    Err(error) => {
                        tracing::error!("hosts: {error}");
                        continue;
                    }
                };
                if heartbeat.elapsed() > Duration::from_secs(10) {
                    for mut host in hosts.clone() {
                        if host.id == "local" {
                            host.observed_at = now();
                            let _ = engine.store.upsert_host(host);
                        } else if host.kind == "peer" {
                            let worker = engine.clone();
                            tokio::spawn(async move {
                                let _ = crate::peer::refresh(&worker, host).await;
                            });
                        }
                    }
                    heartbeat = tokio::time::Instant::now();
                }
                let active = engine
                    .active
                    .lock()
                    .await
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                if active.len() >= 32 {
                    continue;
                }
                let runs = match engine.store.dispatch_runs() {
                    Ok(runs) => runs,
                    Err(error) => {
                        tracing::error!("dispatch: {error}");
                        continue;
                    }
                };
                // Reattach existing remote observers before starting more model processes.
                let mut claimed = runs
                    .iter()
                    .find(|r| {
                        r.host_id != "local"
                            && r.origin == Origin::Managed
                            && r.state.active()
                            && !active.contains(&r.id)
                    })
                    .cloned();
                if claimed.is_none()
                    && runs
                        .iter()
                        .filter(|r| active.contains(&r.id) && r.state == RunState::Running)
                        .count()
                        < 8
                {
                    for host in hosts
                        .iter()
                        .filter(|h| h.id == "local" || (h.kind == "peer" && h.connected))
                    {
                        match engine.store.claim_next_excluding(&host.id, &active) {
                            Ok(Some(run)) => {
                                claimed = Some(run);
                                break;
                            }
                            Err(error) => tracing::error!("dispatch: {error}"),
                            _ => (),
                        }
                    }
                }
                match Ok::<_, bibi_core::Error>(claimed) {
                    Ok(Some(run)) => {
                        let (tx, rx) = mpsc::channel(8);
                        engine.active.lock().await.insert(run.id.clone(), tx);
                        let worker = engine.clone();
                        tokio::spawn(async move {
                            if let Err(error) = worker.execute(run.clone(), rx).await {
                                if worker
                                    .store
                                    .run(&run.id)
                                    .is_ok_and(|r| r.session_key.is_some() || r.host_id != "local")
                                {
                                    let _ = worker.store.uncertain(&run.id, &error.to_string());
                                } else {
                                    let _ = worker.store.fail(&run.id, &error.to_string(), false);
                                }
                            }
                            let _ = worker.store.disconnect_subagents(&run.id);
                            worker.active.lock().await.remove(&run.id);
                        });
                    }
                    Err(e) => tracing::error!("dispatch: {e}"),
                    _ => (),
                }
            }
        });
        Ok(())
    }
    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Relaxed)
    }
    pub async fn stop(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        let controls = self
            .active
            .lock()
            .await
            .iter()
            .filter(|(id, _)| self.store.run(id).is_ok_and(|r| r.host_id == "local"))
            .map(|(_, tx)| tx.clone())
            .collect::<Vec<_>>();
        for control in controls {
            let _ = control.send(Control::Interrupt).await;
        }
        for _ in 0..25 {
            if self.active.lock().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        self.codex_sessions.clear().await;
        self.claude_sessions.clear().await;
    }
    async fn execute(&self, run: Run, controls: mpsc::Receiver<Control>) -> Result<()> {
        if run.host_id != "local" {
            return crate::peer::execute(self, run, controls).await;
        }
        let configured = run
            .provider_id
            .as_deref()
            .map(|id| self.configured(id))
            .transpose()?;
        let engine = configured.as_ref().unwrap_or(self);
        match run.provider {
            Provider::Mock => engine.mock(run, controls).await,
            Provider::Claude => crate::providers::claude::execute(engine, run, controls).await,
            Provider::Codex => crate::providers::codex::execute(engine, run, controls).await,
            Provider::Ollama => crate::providers::ollama::execute(engine, run, controls).await,
            Provider::OpenAi => crate::providers::openai::execute(engine, run, controls).await,
            Provider::Command => crate::providers::command::execute(engine, run, controls).await,
        }
    }
    async fn mock(&self, run: Run, mut controls: mpsc::Receiver<Control>) -> Result<()> {
        let session = crate::providers::previous_session(self, &run)?.unwrap_or_else(|| id("mock"));
        self.store.delivered(&run.id, &session, &id("turn"))?;
        let result = format!(
            "모의 실행 · {}\n\n{}\n\n맥락 버전 {} · 이전 실행 {}\n필수 제약 {}개를 유지했습니다. 실제 모델 호출과 파일 변경은 없습니다.",
            run.role,
            run.context.question,
            run.context_revision,
            run.context.source_runs.len(),
            run.context.constraints.len()
        );
        let chunks: Vec<String> = result
            .chars()
            .collect::<Vec<_>>()
            .chunks(18)
            .map(|c| c.iter().collect())
            .collect();
        let stream = id("message");
        let mut full = String::new();
        for chunk in chunks {
            tokio::select! {
                _=tokio::time::sleep(Duration::from_millis(220))=>{},
                control=controls.recv()=>match control {
                    Some(Control::Interrupt)=>{self.store.fail(&run.id,"사용자가 중단했습니다.",true)?;return Ok(())},
                    Some(Control::Respond{reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("모의 실행에는 승인 요청이 없습니다.")));},
                    None=>bail!("실행 제어 채널이 종료되었습니다."),
                }
            }
            for input in self.store.pending_inputs(&run.id)? {
                self.store.input_state(&input.id, "sending")?;
                self.store.input_state(&input.id, "delivered")?;
                let note = format!("\n[현재 작업 입력] {}\n", input.text);
                self.store
                    .append_output(&run.id, &stream, "assistant", &note)?;
                full.push_str(&note);
            }
            self.store
                .append_output(&run.id, &stream, "assistant", &chunk)?;
            full.push_str(&chunk);
        }
        self.store.complete(&run.id, &full, UsageStats::default())?;
        Ok(())
    }
    pub async fn interrupt(&self, run_id: &str) -> Result<Run> {
        if let Some(run) = self.store.cancel_queued(run_id)? {
            return Ok(run);
        }
        let run = self.store.run(run_id)?;
        if run.origin != Origin::Managed {
            bail!("외부 실행을 중단할 수 없습니다.");
        }
        let sender = self
            .active
            .lock()
            .await
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("현재 호스트에 연결된 실행이 아닙니다."))?;
        sender.send(Control::Interrupt).await?;
        Ok(run)
    }
    pub async fn respond(&self, approval_id: &str, value: Value) -> Result<()> {
        let approval = self.store.approval(approval_id)?;
        if approval.state != "pending" {
            bail!("이미 응답한 요청입니다.");
        }
        let sender = self
            .active
            .lock()
            .await
            .get(&approval.run_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("실행이 연결되어 있지 않습니다."))?;
        let (tx, rx) = oneshot::channel();
        sender
            .send(Control::Respond {
                approval_id: approval_id.into(),
                value,
                reply: tx,
            })
            .await?;
        tokio::time::timeout(Duration::from_secs(20), rx).await???;
        Ok(())
    }
}
