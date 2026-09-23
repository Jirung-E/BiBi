mod preferences;

use crate::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Unsupported(String),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("저장소 잠금을 획득하지 못했습니다.")]
    Poisoned,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
}

fn get<T: DeserializeOwned>(c: &Connection, kind: &str, key: &str) -> Result<Option<T>> {
    let text: Option<String> = c
        .query_row(
            "SELECT data FROM entities WHERE kind=?1 AND id=?2",
            params![kind, key],
            |r| r.get(0),
        )
        .optional()?;
    text.map(|s| serde_json::from_str(&s).map_err(Error::from))
        .transpose()
}
fn required<T: DeserializeOwned>(c: &Connection, kind: &str, key: &str) -> Result<T> {
    get(c, kind, key)?.ok_or_else(|| Error::NotFound(format!("{kind}: {key}")))
}
fn list<T: DeserializeOwned>(c: &Connection, kind: &str, owner: Option<&str>) -> Result<Vec<T>> {
    let sql = if kind == "message" {
        "SELECT data FROM entities WHERE kind=?1 AND (?2 IS NULL OR owner=?2) ORDER BY created_at,rowid"
    } else {
        "SELECT data FROM entities WHERE kind=?1 AND (?2 IS NULL OR owner=?2) ORDER BY created_at,id"
    };
    let mut stmt = c.prepare(sql)?;
    let rows = stmt.query_map(params![kind, owner], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
fn dispatch_runs(c: &Connection, host: Option<&str>) -> Result<Vec<Run>> {
    let mut stmt=c.prepare("SELECT data FROM entities WHERE kind='run' AND json_extract(data,'$.state') IN ('queued','running','waiting_user','waiting_expert','disconnected','uncertain') AND (?1 IS NULL OR owner=?1) ORDER BY created_at,id")?;
    let rows = stmt.query_map([host], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
fn put<T: Serialize>(
    c: &Connection,
    kind: &str,
    key: &str,
    owner: &str,
    created: i64,
    value: &T,
) -> Result<()> {
    c.execute(
        "INSERT INTO entities(kind,id,owner,created_at,data) VALUES(?1,?2,?3,?4,?5)
        ON CONFLICT(kind,id) DO UPDATE SET owner=excluded.owner,data=excluded.data",
        params![kind, key, owner, created, serde_json::to_string(value)?],
    )?;
    Ok(())
}
fn emit<T: Serialize>(c: &Connection, kind: &str, data: &T) -> Result<()> {
    c.execute(
        "INSERT INTO events(id,kind,created_at,data) VALUES(?1,?2,?3,?4)",
        params![id("evt"), kind, now(), serde_json::to_string(data)?],
    )?;
    Ok(())
}
fn save_run(c: &Connection, run: &Run) -> Result<()> {
    let mut run = run.clone();
    if let Some(title) = get::<String>(c, "session_title", run.session_id())? {
        run.title = title;
    }
    put(c, "run", &run.id, &run.host_id, run.created_at, &run)?;
    emit(c, "run", &run)
}
fn message(c: &Connection, run: &str, role: &str, text: &str) -> Result<Message> {
    let m = Message {
        id: id("msg"),
        run_id: run.into(),
        role: role.into(),
        text: text.into(),
        created_at: now(),
    };
    put(c, "message", &m.id, run, m.created_at, &m)?;
    emit(c, "message", &m)?;
    Ok(m)
}

fn initial_message(c: &Connection, run: &Run) -> Result<()> {
    let m = Message {
        id: format!("initial:{}", run.id),
        run_id: run.id.clone(),
        role: "user".into(),
        text: run.context.question.clone(),
        created_at: run.created_at,
    };
    put(c, "message", &m.id, &run.id, m.created_at, &m)?;
    emit(c, "message", &m)
}

fn settle_controls(c: &Connection, run_id: &str) -> Result<()> {
    for mut input in list::<PendingInput>(c, "input", Some(run_id))? {
        let next = match input.state.as_str() {
            "accepted" => "failed",
            "sending" => "uncertain",
            _ => continue,
        };
        input.state = next.into();
        put(c, "input", &input.id, run_id, input.created_at, &input)?;
        emit(c, "input", &input)?;
    }
    for mut approval in list::<Approval>(c, "approval", Some(run_id))? {
        let next = match approval.state.as_str() {
            "pending" => "cancelled",
            "sending" => "uncertain",
            _ => continue,
        };
        approval.state = next.into();
        put(
            c,
            "approval",
            &approval.id,
            run_id,
            approval.created_at,
            &approval,
        )?;
        emit(c, "approval", &approval)?;
    }
    Ok(())
}

fn descendants(c: &Connection, parent: &Run) -> Result<Vec<Run>> {
    let candidates = list::<Run>(c, "run", None)?;
    let mut ids = vec![parent.id.clone()];
    let mut found = Vec::new();
    loop {
        let next = candidates
            .iter()
            .filter(|r| {
                r.work_id == parent.work_id
                    && r.continued_from.is_none()
                    && !ids.contains(&r.id)
                    && r.parent_run_id.as_ref().is_some_and(|id| ids.contains(id))
            })
            .cloned()
            .collect::<Vec<_>>();
        if next.is_empty() {
            break;
        }
        for child in next {
            ids.push(child.id.clone());
            found.push(child);
        }
    }
    Ok(found)
}
fn conversation_messages(c: &Connection, run: &Run) -> Result<Vec<Message>> {
    let mut chain = vec![run.clone()];
    while let Some(parent) = chain.last().unwrap().continued_from.as_deref() {
        let previous: Run = required(c, "run", parent)?;
        if previous.session_id() != run.session_id()
            || previous.work_id != run.work_id
            || chain.iter().any(|r| r.id == previous.id)
            || chain.len() >= 10000
        {
            return Err(Error::Conflict(
                "세션 대화 연결이 일치하지 않습니다.".into(),
            ));
        }
        chain.push(previous);
    }
    let mut messages = Vec::new();
    for run in chain.into_iter().rev() {
        messages.extend(list::<Message>(c, "message", Some(&run.id))?);
    }
    Ok(messages)
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }
    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }
    fn from_connection(c: Connection) -> Result<Self> {
        c.busy_timeout(Duration::from_secs(5))?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS entities(
                kind TEXT NOT NULL,id TEXT NOT NULL,owner TEXT NOT NULL,created_at INTEGER NOT NULL,
                data TEXT NOT NULL,PRIMARY KEY(kind,id));
            CREATE INDEX IF NOT EXISTS entities_owner ON entities(kind,owner,created_at);
            CREATE TABLE IF NOT EXISTS submissions(id TEXT PRIMARY KEY,digest TEXT NOT NULL,receipt TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,
                kind TEXT NOT NULL,created_at INTEGER NOT NULL,data TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS runs_dispatch ON entities(json_extract(data,'$.state'),owner,created_at) WHERE kind='run';
            CREATE INDEX IF NOT EXISTS runs_work ON entities(json_extract(data,'$.work_id'),created_at) WHERE kind='run';
            CREATE INDEX IF NOT EXISTS transmissions_request ON entities(json_extract(data,'$.request_id'),created_at) WHERE kind='transmission';
            PRAGMA user_version=1;")?;
        c.execute("INSERT OR IGNORE INTO entities(kind,id,owner,created_at,data) VALUES('setting','server_id','',?1,?2)",params![now(),serde_json::to_string(&id("server"))?])?;
        Ok(Self {
            connection: Arc::new(Mutex::new(c)),
        })
    }
    fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let c = self.connection.lock().map_err(|_| Error::Poisoned)?;
        f(&c)
    }
    fn write<T>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut c = self.connection.lock().map_err(|_| Error::Poisoned)?;
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.read(|c| get(c, "setting", key))
    }
    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.write(|c| put(c, "setting", key, "", now(), value))
    }
    pub fn add_project(&self, project: Project) -> Result<Project> {
        if project.id.trim().is_empty()
            || project.name.trim().is_empty()
            || project.workspace.is_empty()
        {
            return Err(Error::Invalid(
                "프로젝트 이름과 작업 경로가 필요합니다.".into(),
            ));
        }
        self.write(|c| {
            if get::<Project>(c, "project", &project.id)?.is_some() {
                return Err(Error::Conflict("이미 등록된 프로젝트입니다.".into()));
            }
            put(c, "project", &project.id, "", now(), &project)?;
            emit(c, "project", &project)?;
            Ok(project)
        })
    }
    pub fn server_id(&self) -> Result<String> {
        self.read(|c| required(c, "setting", "server_id"))
    }
    pub fn hosts(&self) -> Result<Vec<Host>> {
        self.read(|c| list(c, "host", None))
    }
    pub fn quotas(&self) -> Result<Vec<Quota>> {
        self.read(|c| list(c, "quota", None))
    }
    pub fn dispatch_runs(&self) -> Result<Vec<Run>> {
        self.read(|c| dispatch_runs(c, None))
    }
    pub fn work_runs(&self, work_id: &str) -> Result<Vec<Run>> {
        self.read(|c|{
            let mut stmt=c.prepare("SELECT data FROM entities WHERE kind='run' AND json_extract(data,'$.work_id')=?1 ORDER BY created_at,id")?;
            let rows=stmt.query_map([work_id],|r|r.get::<_,String>(0))?;
            rows.map(|row|Ok(serde_json::from_str(&row?)?)).collect()
        })
    }
    pub fn request_transmissions(&self, request_id: &str) -> Result<Vec<Transmission>> {
        self.read(|c|{
            let mut stmt=c.prepare("SELECT data FROM entities WHERE kind='transmission' AND json_extract(data,'$.request_id')=?1 ORDER BY created_at,id")?;
            let rows=stmt.query_map([request_id],|r|r.get::<_,String>(0))?;
            rows.map(|row|Ok(serde_json::from_str(&row?)?)).collect()
        })
    }
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.read(|c| list(c, "project", None))
    }
    pub fn project(&self, id: &str) -> Result<Project> {
        self.read(|c| required(c, "project", id))
    }
    pub fn work(&self, id: &str) -> Result<Work> {
        self.read(|c| required(c, "work", id))
    }
    pub fn run(&self, id: &str) -> Result<Run> {
        self.read(|c| required(c, "run", id))
    }
    pub fn migrate_sessions(&self) -> Result<()> {
        self.write(|c| {
            let runs: Vec<Run> = list(c, "run", None)?;
            for mut run in runs.iter().filter(|r| r.session_id.is_empty()).cloned() {
                // Old fresh executions remain separate; their histories must not be invented.
                run.session_id = run.id.clone();
                run.parent_session_id = run
                    .parent_run_id
                    .as_ref()
                    .and_then(|id| runs.iter().find(|r| &r.id == id))
                    .map(|r| r.session_id().to_owned());
                run.agent_kind = if run.role.starts_with("전문가:") {
                    "expert"
                } else {
                    "session"
                }
                .into();
                run.capabilities = if run.origin == Origin::Managed {
                    Capabilities::managed(&run.provider)
                } else {
                    Capabilities::external(&run.provider)
                };
                save_run(c, &run)?;
            }
            Ok(())
        })
    }
    pub fn observe_subagent(&self, parent_id: &str, update: SubagentUpdate) -> Result<Run> {
        self.write(|c| {
            let parent: Run = required(c, "run", parent_id)?;
            let key = format!(
                "agent_{:x}",
                Sha256::digest(format!("{}:{}", parent.session_id(), update.native_id))
            );
            let existing: Option<Run> = get(c, "run", &key)?;
            let is_new = existing.is_none();
            let mut child = existing.unwrap_or_else(|| {
                let mut r = parent.clone();
                r.id = key.clone();
                r.session_id = key.clone();
                r.continued_from = None;
                r.parent_session_id = Some(parent.session_id().into());
                r.parent_run_id = Some(parent.id.clone());
                r.request_id = id("agent_request");
                r.agent_kind = "subagent".into();
                r.role = "서브에이전트".into();
                r.title = if update.title.is_empty() {
                    "서브에이전트".into()
                } else {
                    update.title.clone()
                };
                r.origin = Origin::External;
                r.capabilities = Capabilities::external(&parent.provider);
                r.capabilities.stream_output = Capability::yes();
                r.session_key = Some(update.native_id.clone());
                r.turn_id = None;
                r.state = RunState::Running;
                r.stats = UsageStats::default();
                r.activity = None;
                r.error = None;
                r.created_at = now();
                r.context.question = update.prompt.clone().unwrap_or_default();
                r
            });
            if !update.title.is_empty() {
                child.title = update.title;
            }
            if let Some(state) = update.state
                && (!child.state.terminal() || update.started)
            {
                child.state = state;
            }
            child.phase = match child.state {
                RunState::Completed => "완료",
                RunState::Failed => "실패",
                RunState::Interrupted => "중단",
                RunState::Disconnected => "관측 연결 종료",
                _ => "서브에이전트 실행 중",
            }
            .into();
            child.observation_source = "native_subagent".into();
            child.updated_at = now();
            child.observed_at = now();
            if let Some(stats) = update.stats {
                child.stats = stats;
            }
            save_run(c, &child)?;
            for (role, text) in [("user", update.prompt), ("assistant", update.text)] {
                if let Some(text) = text.filter(|v| !v.is_empty()) {
                    let m = Message {
                        id: format!("{}:{}:{role}", child.id, update.event_id),
                        run_id: child.id.clone(),
                        role: role.into(),
                        text,
                        created_at: now(),
                    };
                    if get::<Message>(c, "message", &m.id)?.is_none() {
                        put(c, "message", &m.id, &child.id, m.created_at, &m)?;
                        emit(c, "message", &m)?;
                    }
                    let edge = Transmission {
                        id: format!("{}:{}:{role}", child.id, update.event_id),
                        from_run_id: Some(if role == "user" {
                            parent.id.clone()
                        } else {
                            child.id.clone()
                        }),
                        to_run_id: if role == "user" {
                            child.id.clone()
                        } else {
                            parent.id.clone()
                        },
                        request_id: child.request_id.clone(),
                        response_id: None,
                        kind: if role == "user" { "request" } else { "reply" }.into(),
                        sent_at: now(),
                    };
                    if get::<Transmission>(c, "transmission", &edge.id)?.is_none() {
                        put(
                            c,
                            "transmission",
                            &edge.id,
                            &child.work_id,
                            edge.sent_at,
                            &edge,
                        )?;
                        emit(c, "transmission", &edge)?;
                    }
                }
            }
            if is_new && child.context.question.is_empty() {
                message(
                    c,
                    &child.id,
                    "system",
                    "런타임이 보고한 서브에이전트입니다. 공개된 메시지만 표시합니다.",
                )?;
            }
            Ok(child)
        })
    }
    pub fn disconnect_subagents(&self, parent_id: &str) -> Result<()> {
        self.write(|c| {
            let parent: Run = required(c, "run", parent_id)?;
            for mut child in descendants(c, &parent)?
                .into_iter()
                .filter(|r| r.agent_kind == "subagent" && !r.state.terminal())
            {
                child.state = RunState::Disconnected;
                child.phase = "관측 연결 종료".into();
                child.updated_at = now();
                save_run(c, &child)?;
            }
            Ok(())
        })
    }
    pub fn receipt(&self, id: &str) -> Result<Option<Receipt>> {
        self.read(|c| {
            let value: Option<String> = c
                .query_row("SELECT receipt FROM submissions WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            value
                .map(|s| serde_json::from_str(&s).map_err(Error::from))
                .transpose()
        })
    }
    pub fn submit(&self, request: Submission) -> Result<Receipt> {
        if request.submission_id.is_empty()
            || request.submission_id.len() > 128
            || request.question.trim().is_empty()
            || request.question.len() > 131_072
        {
            return Err(Error::Invalid(
                "전송 ID와 128 KiB 이하의 질문이 필요합니다.".into(),
            ));
        }
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&request)?));
        self.write(|c| {
            let old: Option<(String, String)> = c
                .query_row(
                    "SELECT digest,receipt FROM submissions WHERE id=?1",
                    [&request.submission_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((previous, receipt)) = old {
                if previous != digest {
                    return Err(Error::Conflict(
                        "같은 전송 ID를 다른 대상이나 내용에 사용할 수 없습니다.".into(),
                    ));
                }
                return Ok(serde_json::from_str(&receipt)?);
            }
            let configured: Option<ProviderConfig> = request.provider_id.as_deref()
                .map(|id| preferences::connection(c,id)).transpose()?;
            if configured.as_ref().is_some_and(|p| p.adapter != request.provider || p.host_id != request.host_id) {
                return Err(Error::Conflict("제공자 연결 방식이 변경되었습니다.".into()));
            }
            let project: Project = required(c, "project", &request.project_key)?;
            let host: Host = required(c, "host", &request.host_id)?;
            if !host.providers.contains(&request.provider) {
                return Err(Error::Unsupported(
                    "이 호스트가 해당 서비스를 지원하지 않습니다.".into(),
                ));
            }
            let workspace = if host.id == "local" {
                project.workspace.clone()
            } else {
                get::<String>(
                    c,
                    "setting",
                    &format!("host_workspace:{}:{}", host.id, project.id),
                )?
                .ok_or_else(|| {
                    Error::Invalid("이 호스트의 프로젝트 작업 경로를 먼저 등록하세요.".into())
                })?
            };
            let target: Option<Run> = request
                .target_run_id
                .as_deref()
                .map(|id| required(c, "run", id))
                .transpose()?;
            if let Some(t) = &target
                && (t.project_key != project.id
                    || request.work_id.as_ref().is_some_and(|w| w != &t.work_id))
            {
                return Err(Error::Conflict(
                    "선택한 실행의 프로젝트·업무가 전송 대상과 다릅니다.".into(),
                ));
            }
            if request.mode == SubmitMode::Continue {
                let t = target.as_ref().ok_or_else(|| Error::Invalid("이어갈 세션을 선택하세요.".into()))?;
                if t.origin != Origin::Managed || !t.capabilities.continue_session.supported
                    || !t.state.terminal() || t.state == RunState::Uncertain {
                    return Err(Error::Conflict("현재 세션의 응답·복구 확인이 끝난 뒤 이어갈 수 있습니다.".into()));
                }
                if t.provider != request.provider || (t.provider_id.is_some() && t.provider_id != request.provider_id) || t.host_id != request.host_id
                    || t.model != request.model || t.role != request.role
                    || t.read_only != (request.read_only || request.provider == Provider::Ollama)
                    || t.turn_id != request.expected_turn_id {
                    return Err(Error::Conflict("이어가기 대상·모델·권한이 변경되었습니다. 설정을 바꾸려면 새 세션을 선택하세요.".into()));
                }
                if get::<String>(c,"hidden_session",t.session_id())?.is_some() {return Err(Error::Conflict("제거한 세션은 복원한 뒤 이어가세요.".into()));}
                if t.session_key.is_none() {
                    return Err(Error::Conflict("복원할 모델 세션이 없습니다. 새 세션으로 시작하세요.".into()));
                }
                if list::<Run>(c,"run",None)?.iter().any(|r| r.continued_from.as_ref() == Some(&t.id)) {
                    return Err(Error::Conflict("이 세션에 새 요청이 이미 접수되었습니다. 최신 대화를 여세요.".into()));
                }
            }
            let receipt = if request.mode == SubmitMode::Steer {
                let t =
                    target.ok_or_else(|| Error::Invalid("현재 실행 대상이 필요합니다.".into()))?;
                if t.origin != Origin::Managed
                    || t.state != RunState::Running
                    || !t.capabilities.send_to_active.supported
                {
                    return Err(Error::Conflict(
                        "현재 입력을 받을 수 있는 실행이 아닙니다.".into(),
                    ));
                }
                if request.host_id != t.host_id
                    || request.provider != t.provider || request.provider_id != t.provider_id
                    || request.expected_turn_id.is_none()
                    || request.expected_turn_id != t.turn_id
                {
                    return Err(Error::Conflict(
                        "실행 턴·호스트·서비스가 변경되었습니다. 대상을 다시 확인하세요.".into(),
                    ));
                }
                let input = PendingInput {
                    id: request.submission_id.clone(),
                    run_id: t.id.clone(),
                    expected_turn_id: t.turn_id.clone().unwrap(),
                    text: request.question.clone(),
                    state: "accepted".into(),
                    created_at: now(),
                };
                put(c, "input", &input.id, &t.id, input.created_at, &input)?;
                emit(c, "input", &input)?;
                Receipt {
                    submission_id: request.submission_id.clone(),
                    run_id: t.id,
                    request_id: t.request_id,
                    work_id: t.work_id,
                    status: "accepted".into(),
                }
            } else {
                let work_key = request
                    .work_id
                    .as_deref()
                    .or(target.as_ref().map(|t| t.work_id.as_str()));
                let mut work: Work = if let Some(w) = work_key {
                    required(c, "work", w)?
                } else {
                    Work {
                        id: id("work"),
                        project_key: project.id.clone(),
                        conversation_id: id("conversation"),
                        title: request
                            .title
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| request.question.chars().take(70).collect()),
                        goal: request.question.clone(),
                        context_revision: 1,
                        constraints: project.constraints.clone(),
                        decisions: vec![],
                        performed_actions: vec![],
                        open_questions: vec![],
                        references: vec![],
                        created_at: now(),
                    }
                };
                if work.project_key != project.id {
                    return Err(Error::Conflict("다른 프로젝트의 업무입니다.".into()));
                }
                if work_key.is_some()
                    && request.expected_context_revision != Some(work.context_revision)
                {
                    return Err(Error::Conflict(
                        "업무 맥락이 변경되었습니다. 최신 버전을 확인하세요.".into(),
                    ));
                }
                let request_id = id("request");
                let run_id = id("run");
                let mut references = work.references.clone();
                let prior: Option<InboxEntry> = if let Some(t) = &target {
                    list::<InboxEntry>(c, "inbox", Some(&work.conversation_id))?
                        .into_iter()
                        .rev()
                        .find(|e| e.from_run_id == t.id)
                } else {
                    None
                };
                let mut previous_answer_excerpt = None;
                let mut excerpt_truncated = false;
                if let Some(entry) = &prior {
                    previous_answer_excerpt = Some(entry.result.chars().take(6000).collect());
                    excerpt_truncated = entry.result.chars().count() > 6000;
                    references.push(Evidence {
                        text: "선택한 답변 원문".into(),
                        source: format!("bibi://inbox/{}", entry.response_id),
                        revision: entry.context_revision.to_string(),
                    });
                }
                if previous_answer_excerpt.is_none()
                    && let Some(t) = &target
                    && let Some(m) = list::<Message>(c, "message", Some(&t.id))?
                        .into_iter()
                        .rev()
                        .find(|m| m.role == "assistant")
                {
                    previous_answer_excerpt = Some(m.text.chars().take(6000).collect());
                    excerpt_truncated = m.text.chars().count() > 6000;
                    references.push(Evidence {
                        text: "선택한 실행의 답변 원문".into(),
                        source: format!("bibi://runs/{}/messages/{}", t.id, m.id),
                        revision: t.updated_at.to_string(),
                    });
                }
                for constraint in &project.constraints {
                    if !work.constraints.contains(constraint) {
                        work.constraints.push(constraint.clone());
                    }
                }
                let context = ContextPacket {
                    schema_version: 1,
                    project_key: project.id.clone(),
                    work_id: work.id.clone(),
                    conversation_id: work.conversation_id.clone(),
                    request_id: request_id.clone(),
                    parent_request_id: target.as_ref().map(|t| t.request_id.clone()),
                    reply_to_response_id: prior.as_ref().map(|e| e.response_id.clone()),
                    context_revision: work.context_revision,
                    role: request.role.clone(),
                    question: request.question.clone(),
                    goal: work.goal.clone(),
                    constraints: work.constraints.clone(),
                    decisions: work.decisions.clone(),
                    performed_actions: work.performed_actions.clone(),
                    open_questions: work.open_questions.clone(),
                    references,
                    source_runs: target
                        .as_ref()
                        .map(|t| vec![t.id.clone()])
                        .unwrap_or_default(),
                    previous_answer_excerpt,
                    excerpt_truncated,
                    workspace: workspace.clone(),
                    reply_to: format!("bibi://inbox/{}/{}", work.conversation_id, request_id),
                };
                if serde_json::to_vec(&context)?.len() > 262_144 {
                    return Err(Error::Invalid(
                        "인계 자료가 256 KiB를 초과합니다. 제약을 유지하며 명시적으로 정리하세요."
                            .into(),
                    ));
                }
                let continuing = request.mode == SubmitMode::Continue;
                let run = Run {
                    id: run_id.clone(),
                    session_id: if continuing { target.as_ref().unwrap().session_id().to_owned() } else { id("session") },
                    continued_from: if continuing { target.as_ref().map(|t|t.id.clone()) } else { None },
                    parent_session_id: if continuing { target.as_ref().and_then(|t| t.parent_session_id.clone()) } else { target.as_ref().map(|t|t.session_id().to_owned()) },
                    agent_kind: if continuing { target.as_ref().unwrap().agent_kind.clone() } else if request.role.starts_with("전문가:") { "expert".into() } else { "session".into() },
                    project_key: project.id,
                    work_id: work.id.clone(),
                    conversation_id: work.conversation_id.clone(),
                    request_id: request_id.clone(),
                    parent_run_id: target.as_ref().map(|t| t.id.clone()),
                    context_revision: work.context_revision,
                    role: request.role.clone(),
                    title: if continuing { target.as_ref().unwrap().title.clone() } else { request.question.chars().take(90).collect() },
                    provider: request.provider.clone(),
                    provider_id: request.provider_id.clone(),
                    model: request.model.clone(),
                    host_id: request.host_id.clone(),
                    state: RunState::Queued,
                    phase: "접수됨".into(),
                    wait_reason: None,
                    observation_source: "service".into(),
                    created_at: now(),
                    updated_at: now(),
                    observed_at: now(),
                    session_key: None,
                    turn_id: None,
                    origin: Origin::Managed,
                    workspace,
                    read_only: request.read_only || request.provider == Provider::Ollama,
                    capabilities: Capabilities::managed(&request.provider),
                    context,
                    stats: UsageStats::default(),
                    runtime: if continuing { target.as_ref().unwrap().runtime.clone() } else { RuntimeMetadata { provider_name: configured.as_ref().map(|p| p.name.clone()), ..Default::default() } },
                    activity: None,
                    error: None,
                };
                put(
                    c,
                    "work",
                    &work.id,
                    &work.project_key,
                    work.created_at,
                    &work,
                )?;
                emit(c, "work", &work)?;
                save_run(c, &run)?;
                initial_message(c, &run)?;
                Receipt {
                    submission_id: request.submission_id.clone(),
                    run_id,
                    request_id,
                    work_id: work.id,
                    status: "accepted".into(),
                }
            };
            if let Some(provider_id) = &request.provider_id {
                preferences::remember(c, &ModelSelection { provider_id: provider_id.clone(), model: request.model.clone() }, true)?;
            }
            c.execute(
                "INSERT INTO submissions(id,digest,receipt) VALUES(?1,?2,?3)",
                params![
                    request.submission_id,
                    digest,
                    serde_json::to_string(&receipt)?
                ],
            )?;
            Ok(receipt)
        })
    }
    pub fn update_context(&self, work_id: &str, update: ContextUpdate) -> Result<Work> {
        self.write(|c| {
            let mut w: Work = required(c, "work", work_id)?;
            if w.context_revision != update.expected_revision {
                return Err(Error::Conflict("업무 맥락 버전이 변경되었습니다.".into()));
            }
            let p: Project = required(c, "project", &w.project_key)?;
            if p.constraints
                .iter()
                .any(|v| !update.constraints.contains(v))
            {
                return Err(Error::Invalid(
                    "프로젝트 필수 제약을 제거할 수 없습니다.".into(),
                ));
            }
            if update.goal.trim().is_empty() {
                return Err(Error::Invalid("완료 목표가 필요합니다.".into()));
            }
            w.context_revision += 1;
            w.goal = update.goal;
            w.constraints = update.constraints;
            w.decisions = update.decisions;
            w.performed_actions = update.performed_actions;
            w.open_questions = update.open_questions;
            w.references = update.references;
            put(c, "work", &w.id, &w.project_key, w.created_at, &w)?;
            emit(c, "work", &w)?;
            Ok(w)
        })
    }
    pub fn claim_next(&self, host_id: &str) -> Result<Option<Run>> {
        self.claim_next_excluding(host_id, &[])
    }
    pub fn claim_next_excluding(&self, host_id: &str, excluded: &[String]) -> Result<Option<Run>> {
        self.write(|c| {
            let runs = dispatch_runs(c, Some(host_id))?;
            let candidate = runs.iter().find(|r| {
                r.origin == Origin::Managed
                    && r.state == RunState::Queued
                    && !excluded.contains(&r.id)
                    && r.continued_from
                        .as_ref()
                        .is_none_or(|p| !excluded.contains(p))
                    && (r.read_only
                        || !runs.iter().any(|a| {
                            !a.read_only
                                && a.workspace == r.workspace
                                && (a.state.active() || a.state == RunState::Uncertain)
                        }))
            });
            let Some(candidate) = candidate else {
                return Ok(None);
            };
            let mut run = candidate.clone();
            run.state = RunState::Running;
            run.phase = "전달 중".into();
            run.updated_at = now();
            run.observed_at = now();
            save_run(c, &run)?;
            Ok(Some(run))
        })
    }
    pub fn runtime_started(&self, run_id: &str, session_key: &str) -> Result<()> {
        self.write(|c| {
            let mut run: Run = required(c, "run", run_id)?;
            if !run.state.active() {
                return Err(Error::Conflict(
                    "실행 시작 전에 상태가 변경되었습니다.".into(),
                ));
            }
            run.session_key = Some(session_key.into());
            run.updated_at = now();
            save_run(c, &run)
        })
    }
    pub fn uncertain(&self, run_id: &str, error: &str) -> Result<Run> {
        self.write(|c| {
            let mut run: Run = required(c, "run", run_id)?;
            if run.state.terminal() {
                return Ok(run);
            }
            run.state = RunState::Uncertain;
            run.phase = "실행 확인 필요".into();
            run.error = Some(error.into());
            run.updated_at = now();
            settle_controls(c, run_id)?;
            save_run(c, &run)?;
            Ok(run)
        })
    }
    pub fn delivered(&self, run_id: &str, session_key: &str, turn_id: &str) -> Result<Run> {
        self.write(|c| {
            let mut r: Run = required(c, "run", run_id)?;
            if r.state != RunState::Running {
                return Err(Error::Conflict("실행 상태가 변경되었습니다.".into()));
            }
            r.session_key = Some(session_key.into());
            r.turn_id = Some(turn_id.into());
            r.phase = "진행 중".into();
            r.updated_at = now();
            r.observed_at = now();
            r.observation_source = "adapter".into();
            save_run(c, &r)?;
            let edge_id = format!("request:{}", r.request_id);
            if get::<Transmission>(c, "transmission", &edge_id)?.is_none() {
                let e = Transmission {
                    id: edge_id,
                    from_run_id: r.parent_run_id.clone(),
                    to_run_id: r.id.clone(),
                    request_id: r.request_id.clone(),
                    response_id: None,
                    kind: "request".into(),
                    sent_at: now(),
                };
                put(c, "transmission", &e.id, &r.work_id, e.sent_at, &e)?;
                emit(c, "transmission", &e)?;
            }
            Ok(r)
        })
    }
    pub fn report(&self, run_id: &str, mut activity: Activity) -> Result<Run> {
        self.write(|c| {
            let mut run: Run = required(c, "run", run_id)?;
            if activity.revision != run.activity.as_ref().map(|a| a.revision + 1).unwrap_or(1) {
                return Err(Error::Conflict("업무 보고 순서가 변경되었습니다.".into()));
            }
            activity.reported_at = now();
            run.activity = Some(activity);
            save_run(c, &run)?;
            Ok(run)
        })
    }
    pub fn observe(
        &self,
        run_id: &str,
        state: RunState,
        phase: &str,
        wait: Option<String>,
    ) -> Result<Run> {
        self.write(|c| {
            let mut r: Run = required(c, "run", run_id)?;
            if r.state.terminal() {
                return Err(Error::Conflict(
                    "종료한 실행의 상태를 덮어쓸 수 없습니다.".into(),
                ));
            }
            r.state = state;
            r.phase = phase.into();
            r.wait_reason = wait;
            r.updated_at = now();
            r.observed_at = now();
            r.observation_source = "adapter".into();
            save_run(c, &r)?;
            Ok(r)
        })
    }
    pub fn append_output(
        &self,
        run_id: &str,
        stream_id: &str,
        role: &str,
        delta: &str,
    ) -> Result<()> {
        if delta.is_empty() {
            return Ok(());
        }
        self.write(|c| {
            let mut r: Run = required(c, "run", run_id)?;
            if r.state.terminal() {
                return Err(Error::Conflict(
                    "종료한 실행에 출력을 추가할 수 없습니다.".into(),
                ));
            }
            if now() - r.observed_at >= 2000 {
                r.observed_at = now();
                r.observation_source = "adapter".into();
                save_run(c, &r)?;
            }
            let mut m: Message = get(c, "message", stream_id)?.unwrap_or(Message {
                id: stream_id.into(),
                run_id: run_id.into(),
                role: role.into(),
                text: String::new(),
                created_at: now(),
            });
            if m.run_id != run_id {
                return Err(Error::Conflict(
                    "출력 ID가 다른 실행에 연결되어 있습니다.".into(),
                ));
            }
            m.text.push_str(delta);
            put(c, "message", &m.id, run_id, m.created_at, &m)?;
            emit(
                c,
                "output",
                &json!({"run_id":run_id,"message_id":stream_id,"role":role,"delta":delta}),
            )?;
            Ok(())
        })
    }
    pub fn set_message(&self, mut value: Message) -> Result<()> {
        self.write(|c| {
            let _: Run = required(c, "run", &value.run_id)?;
            if let Some(old) = get::<Message>(c, "message", &value.id)? {
                if old.run_id != value.run_id {
                    return Err(Error::Conflict("다른 실행의 메시지 ID입니다.".into()));
                }
                value.created_at = old.created_at;
            }
            put(
                c,
                "message",
                &value.id,
                &value.run_id,
                value.created_at,
                &value,
            )?;
            emit(c, "message", &value)
        })
    }
    pub fn add_message(&self, run_id: &str, role: &str, text: &str) -> Result<Message> {
        self.write(|c| {
            let _: Run = required(c, "run", run_id)?;
            message(c, run_id, role, text)
        })
    }
    pub fn usage(&self, run_id: &str, stats: UsageStats) -> Result<()> {
        self.write(|c| {
            let mut run: Run = required(c, "run", run_id)?;
            run.stats = stats;
            save_run(c, &run)
        })
    }
    pub fn complete(&self, run_id: &str, result: &str, stats: UsageStats) -> Result<InboxEntry> {
        self.write(|c| {
            let mut r: Run = required(c, "run", run_id)?;
            let entries: Vec<InboxEntry> = list(c, "inbox", Some(&r.conversation_id))?;
            if let Some(entry) = entries.into_iter().find(|e| e.from_run_id == r.id) {
                if entry.result != result {
                    return Err(Error::Conflict(
                        "저장된 결과와 다른 중복 완료입니다.".into(),
                    ));
                }
                return Ok(entry);
            }
            let late = r.state.terminal();
            let e = InboxEntry {
                response_id: id("response"),
                request_id: r.request_id.clone(),
                from_run_id: r.id.clone(),
                requester_run_id: r.parent_run_id.clone(),
                to_conversation_id: r.conversation_id.clone(),
                context_revision: r.context_revision,
                result: result.into(),
                late,
                created_at: now(),
            };
            put(
                c,
                "inbox",
                &e.response_id,
                &r.conversation_id,
                e.created_at,
                &e,
            )?;
            emit(c, "inbox", &e)?;
            if let Some(parent) = &r.parent_run_id {
                let edge = Transmission {
                    id: format!("reply:{}", e.response_id),
                    from_run_id: Some(r.id.clone()),
                    to_run_id: parent.clone(),
                    request_id: r.request_id.clone(),
                    response_id: Some(e.response_id.clone()),
                    kind: "reply".into(),
                    sent_at: e.created_at,
                };
                put(c, "transmission", &edge.id, &r.work_id, edge.sent_at, &edge)?;
                emit(c, "transmission", &edge)?;
            }
            if !late {
                r.state = RunState::Completed;
                r.phase = "결과 저장됨".into();
                r.wait_reason = None;
                r.stats = stats;
                r.updated_at = now();
                r.observed_at = now();
                settle_controls(c, run_id)?;
                save_run(c, &r)?;
            }
            Ok(e)
        })
    }
    pub fn fail(&self, run_id: &str, error: &str, interrupted: bool) -> Result<Run> {
        self.write(|c| {
            let mut r: Run = required(c, "run", run_id)?;
            if r.state.terminal() {
                return Ok(r);
            }
            r.state = if interrupted {
                RunState::Interrupted
            } else {
                RunState::Failed
            };
            r.phase = if interrupted { "중단됨" } else { "실패" }.into();
            r.error = Some(error.into());
            r.wait_reason = None;
            r.updated_at = now();
            r.observed_at = now();
            settle_controls(c, run_id)?;
            save_run(c, &r)?;
            Ok(r)
        })
    }
    pub fn recover_host(&self, host_id: &str) -> Result<usize> {
        self.write(|c| {
            let runs: Vec<Run> = list(c, "run", Some(host_id))?;
            let mut count = 0;
            for mut run in runs.into_iter().filter(|r| {
                (r.origin == Origin::Managed || r.agent_kind == "subagent") && r.state.active()
            }) {
                run.state = if run.agent_kind == "subagent" {
                    RunState::Disconnected
                } else {
                    RunState::Uncertain
                };
                run.phase = "실행 확인 필요".into();
                run.error = Some(
                    "호스트 연결이 종료되었습니다. 이전 프로세스와 파일 변경을 확인하세요.".into(),
                );
                run.updated_at = now();
                settle_controls(c, &run.id)?;
                save_run(c, &run)?;
                count += 1;
            }
            Ok(count)
        })
    }
    pub fn resolve_uncertain(&self, run_id: &str) -> Result<Run> {
        self.write(|c| {
            let mut r: Run = required(c, "run", run_id)?;
            if r.state != RunState::Uncertain {
                return Err(Error::Conflict("복구 확인 대상이 아닙니다.".into()));
            }
            r.state = RunState::Interrupted;
            r.phase = "사용자가 종료 확인".into();
            r.updated_at = now();
            save_run(c, &r)?;
            Ok(r)
        })
    }
    pub fn cancel_queued(&self, run_id: &str) -> Result<Option<Run>> {
        self.write(|c| {
            let mut run: Run = required(c, "run", run_id)?;
            if run.origin != Origin::Managed || run.state != RunState::Queued {
                return Ok(None);
            }
            run.state = RunState::Interrupted;
            run.phase = "대기 취소됨".into();
            run.updated_at = now();
            save_run(c, &run)?;
            Ok(Some(run))
        })
    }
    pub fn inputs(&self, run_id: &str) -> Result<Vec<PendingInput>> {
        self.read(|c| list(c, "input", Some(run_id)))
    }
    pub fn pending_inputs(&self, run_id: &str) -> Result<Vec<PendingInput>> {
        self.read(|c| {
            Ok(list::<PendingInput>(c, "input", Some(run_id))?
                .into_iter()
                .filter(|i| i.state == "accepted")
                .collect())
        })
    }
    pub fn input_state(&self, input_id: &str, state: &str) -> Result<PendingInput> {
        self.write(|c| {
            let mut i: PendingInput = required(c, "input", input_id)?;
            if i.state == state {
                return Ok(i);
            }
            if !matches!(
                (i.state.as_str(), state),
                ("accepted", "sending")
                    | ("sending", "delivered" | "failed" | "uncertain")
                    | ("accepted", "failed")
            ) {
                return Err(Error::Conflict(
                    "이미 전달을 시도했거나 유효하지 않은 입력 상태입니다.".into(),
                ));
            }
            i.state = state.into();
            put(c, "input", &i.id, &i.run_id, i.created_at, &i)?;
            emit(c, "input", &i)?;
            if state == "delivered" {
                let m = Message {
                    id: format!("input:{}", i.id),
                    run_id: i.run_id.clone(),
                    role: "user".into(),
                    text: i.text.clone(),
                    created_at: now(),
                };
                put(c, "message", &m.id, &m.run_id, m.created_at, &m)?;
                emit(c, "message", &m)?;
                let r: Run = required(c, "run", &i.run_id)?;
                let e = Transmission {
                    id: format!("steer:{}", i.id),
                    from_run_id: None,
                    to_run_id: i.run_id.clone(),
                    request_id: r.request_id,
                    response_id: None,
                    kind: "steer".into(),
                    sent_at: now(),
                };
                put(c, "transmission", &e.id, &r.work_id, e.sent_at, &e)?;
                emit(c, "transmission", &e)?;
            }
            Ok(i)
        })
    }
    pub fn add_approval(&self, a: Approval) -> Result<()> {
        self.write(|c| {
            put(c, "approval", &a.id, &a.run_id, a.created_at, &a)?;
            emit(c, "approval", &a)
        })
    }
    pub fn approval(&self, id: &str) -> Result<Approval> {
        self.read(|c| required(c, "approval", id))
    }
    pub fn approval_state(&self, id: &str, state: &str) -> Result<()> {
        self.write(|c| {
            let mut a: Approval = required(c, "approval", id)?;
            if a.state == state {
                return Ok(());
            }
            if !matches!(
                (a.state.as_str(), state),
                ("pending", "sending") | ("sending", "delivered" | "uncertain")
            ) {
                return Err(Error::Conflict(
                    "이미 응답했거나 유효하지 않은 승인 상태입니다.".into(),
                ));
            }
            a.state = state.into();
            put(c, "approval", &a.id, &a.run_id, a.created_at, &a)?;
            emit(c, "approval", &a)
        })
    }
    pub fn upsert_host(&self, host: Host) -> Result<()> {
        self.write(|c| {
            put(c, "host", &host.id, "", host.observed_at, &host)?;
            emit(c, "host", &host)
        })
    }
    pub fn upsert_quota(&self, quota: Quota) -> Result<()> {
        self.write(|c| {
            put(c, "quota", &quota.id, &quota.host_id, now(), &quota)?;
            emit(c, "quota", &quota)
        })
    }
    pub fn replace_quotas(
        &self,
        provider: &Provider,
        host_id: &str,
        quotas: Vec<Quota>,
    ) -> Result<()> {
        self.write(|c| {
            for old in list::<Quota>(c, "quota", Some(host_id))?
                .into_iter()
                .filter(|q| &q.provider == provider)
            {
                c.execute(
                    "DELETE FROM entities WHERE kind='quota' AND id=?1",
                    [old.id],
                )?;
            }
            for quota in &quotas {
                if &quota.provider != provider || quota.host_id != host_id {
                    return Err(Error::Invalid("한도 범위가 일치하지 않습니다.".into()));
                }
                put(c, "quota", &quota.id, host_id, now(), quota)?;
            }
            emit(
                c,
                "quotas",
                &json!({"provider":provider,"host_id":host_id,"quotas":quotas}),
            )
        })
    }
    pub fn accept_forwarded(&self, mut job: ForwardJob) -> Result<Run> {
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&job)?));
        self.write(|c| {
            let key = format!("forwarded:{}", job.run.id);
            if let Some(old) = get::<Run>(c, "run", &job.run.id)? {
                if get::<String>(c, "setting", &key)?.as_deref() != Some(&digest) {
                    return Err(Error::Conflict(
                        "같은 실행 ID를 다른 원격 작업에 사용할 수 없습니다.".into(),
                    ));
                }
                return Ok(old);
            }
            if job.run.project_key != job.project.id
                || job.run.work_id != job.work.id
                || job.work.project_key != job.project.id
                || job.run.context.request_id != job.run.request_id
                || job.run.conversation_id != job.work.conversation_id
            {
                return Err(Error::Invalid(
                    "원격 작업의 식별자가 일치하지 않습니다.".into(),
                ));
            }
            job.project.workspace = job.run.workspace.clone();
            put(c, "project", &job.project.id, "", now(), &job.project)?;
            emit(c, "project", &job.project)?;
            let previous: Option<Work> = get(c, "work", &job.work.id)?;
            if previous.is_none_or(|w| w.context_revision <= job.work.context_revision) {
                put(
                    c,
                    "work",
                    &job.work.id,
                    &job.project.id,
                    job.work.created_at,
                    &job.work,
                )?;
                emit(c, "work", &job.work)?;
            }
            job.run.host_id = "local".into();
            job.run.state = RunState::Queued;
            job.run.phase = "원격 접수됨".into();
            job.run.origin = Origin::Managed;
            job.run.session_key = None;
            job.run.turn_id = None;
            job.run.observed_at = now();
            job.run.updated_at = now();
            put(c, "setting", &key, "", now(), &digest)?;
            save_run(c, &job.run)?;
            initial_message(c, &job.run)?;
            Ok(job.run)
        })
    }
    pub fn mirror_remote(
        &self,
        host_id: &str,
        detail: RunDetail,
        edges: Vec<Transmission>,
    ) -> Result<Run> {
        self.write(|c| {
            let mut local: Run = required(c, "run", &detail.run.id)?;
            if local.host_id != host_id
                || local.request_id != detail.run.request_id
                || local.work_id != detail.run.work_id
                || local.project_key != detail.run.project_key
                || local.conversation_id != detail.run.conversation_id
            {
                return Err(Error::Conflict(
                    "원격 실행의 소유 호스트·문의가 일치하지 않습니다.".into(),
                ));
            }
            let was_terminal = local.state.terminal();
            let old = local.clone();
            if !was_terminal {
                local.state = detail.run.state;
                local.phase = detail.run.phase;
                local.wait_reason = detail.run.wait_reason;
                local.observed_at = detail.run.observed_at;
                local.updated_at = detail.run.updated_at;
                local.observation_source = format!("host:{host_id}");
                local.session_key = detail.run.session_key;
                local.turn_id = detail.run.turn_id;
                local.model = detail.run.model;
                local.runtime = detail.run.runtime;
                local.stats = detail.run.stats;
                local.activity = detail.run.activity;
                local.error = detail.run.error;
                local.capabilities = detail.run.capabilities;
            }
            if serde_json::to_value(&old)? != serde_json::to_value(&local)? {
                save_run(c, &local)?;
            }
            let mut allowed_requests = vec![local.request_id.clone()];
            let mut parents = vec![local.id.clone()];
            for mut child in detail.children {
                if child.run.project_key != local.project_key
                    || child.run.work_id != local.work_id
                    || child.run.continued_from.is_some()
                    || !child
                        .run
                        .parent_run_id
                        .as_ref()
                        .is_some_and(|id| parents.contains(id))
                {
                    return Err(Error::Conflict(
                        "원격 서브에이전트의 부모·업무가 일치하지 않습니다.".into(),
                    ));
                }
                let existing: Option<Run> = get(c, "run", &child.run.id)?;
                if existing.as_ref().is_some_and(|r| {
                    r.host_id != host_id || r.parent_session_id != child.run.parent_session_id
                }) {
                    return Err(Error::Conflict("원격 서브에이전트 ID 충돌".into()));
                }
                child.run.provider_id = child
                    .run
                    .provider_id
                    .map(|id| remote_provider_id(host_id, &id));
                child.run.host_id = host_id.into();
                child.run.origin = Origin::External;
                child.run.capabilities = Capabilities::external(&child.run.provider);
                child.run.capabilities.stream_output = Capability::yes();
                child.run.observation_source = format!("host:{host_id}:subagent");
                if existing.as_ref().is_none_or(|r| {
                    r.updated_at != child.run.updated_at || r.state != child.run.state
                }) {
                    save_run(c, &child.run)?;
                }
                for message in child.messages {
                    if message.run_id != child.run.id {
                        return Err(Error::Conflict(
                            "원격 서브에이전트 메시지 대상 불일치".into(),
                        ));
                    }
                    let old: Option<Message> = get(c, "message", &message.id)?;
                    if old.as_ref().is_some_and(|m| m.run_id != child.run.id) {
                        return Err(Error::Conflict("원격 메시지 ID 충돌".into()));
                    }
                    if old.as_ref().is_none_or(|m| m.text != message.text) {
                        put(
                            c,
                            "message",
                            &message.id,
                            &child.run.id,
                            message.created_at,
                            &message,
                        )?;
                        emit(c, "message", &message)?;
                    }
                }
                parents.push(child.run.id);
                allowed_requests.push(child.run.request_id);
            }
            for message in detail.messages {
                if message.run_id != local.id {
                    return Err(Error::Invalid("다른 실행의 원격 메시지입니다.".into()));
                }
                let old: Option<Message> = get(c, "message", &message.id)?;
                if old.as_ref().is_some_and(|m| m.run_id != local.id) {
                    return Err(Error::Conflict("메시지 ID 충돌".into()));
                }
                if old.as_ref().is_none_or(|m| m.text != message.text) {
                    put(
                        c,
                        "message",
                        &message.id,
                        &local.id,
                        message.created_at,
                        &message,
                    )?;
                    emit(c, "message", &message)?;
                }
            }
            for mut entry in detail
                .inbox
                .into_iter()
                .filter(|entry| entry.from_run_id == local.id)
            {
                if entry.request_id != local.request_id
                    || entry.to_conversation_id != local.conversation_id
                {
                    return Err(Error::Conflict("수신함 문의 ID 불일치".into()));
                }
                entry.late |= was_terminal && old.state != RunState::Completed;
                if get::<InboxEntry>(c, "inbox", &entry.response_id)?.is_none() {
                    put(
                        c,
                        "inbox",
                        &entry.response_id,
                        &local.conversation_id,
                        entry.created_at,
                        &entry,
                    )?;
                    emit(c, "inbox", &entry)?;
                }
            }
            for input in detail.inputs {
                if input.run_id != local.id {
                    return Err(Error::Invalid("다른 실행의 입력입니다.".into()));
                }
                let old: Option<PendingInput> = get(c, "input", &input.id)?;
                if old.as_ref().is_some_and(|v| {
                    v.run_id != local.id
                        || v.text != input.text
                        || v.expected_turn_id != input.expected_turn_id
                }) {
                    return Err(Error::Conflict("입력 ID 충돌".into()));
                }
                // Acknowledgement by the remote queue is not runtime delivery.
                if old
                    .as_ref()
                    .is_some_and(|v| v.state == "sending" && input.state == "accepted")
                {
                    continue;
                }
                if old.as_ref().is_none_or(|v| v.state != input.state) {
                    put(c, "input", &input.id, &local.id, input.created_at, &input)?;
                    emit(c, "input", &input)?;
                }
            }
            for approval in detail.approvals {
                if approval.run_id != local.id {
                    return Err(Error::Invalid("다른 실행의 승인 요청입니다.".into()));
                }
                let old: Option<Approval> = get(c, "approval", &approval.id)?;
                if old.as_ref().is_none_or(|a| a.state != approval.state) {
                    put(
                        c,
                        "approval",
                        &approval.id,
                        &local.id,
                        approval.created_at,
                        &approval,
                    )?;
                    emit(c, "approval", &approval)?;
                }
            }
            for edge in edges
                .into_iter()
                .filter(|e| allowed_requests.contains(&e.request_id))
            {
                if get::<Transmission>(c, "transmission", &edge.id)?.is_none() {
                    put(
                        c,
                        "transmission",
                        &edge.id,
                        &local.work_id,
                        edge.sent_at,
                        &edge,
                    )?;
                    emit(c, "transmission", &edge)?;
                }
            }
            Ok(local)
        })
    }
    pub fn adopt_remote(&self, host_id: &str, mut run: Run, work: Work) -> Result<bool> {
        self.write(|c| {
            if get::<Run>(c, "run", &run.id)?.is_some() {
                return Ok(false);
            }
            let _: Project = required(c, "project", &run.project_key)?;
            let _: String = required(
                c,
                "setting",
                &format!("host_workspace:{host_id}:{}", run.project_key),
            )?;
            if work.id != run.work_id || work.project_key != run.project_key {
                return Err(Error::Invalid("원격 업무 식별자 불일치".into()));
            }
            if get::<Work>(c, "work", &work.id)?.is_none() {
                put(
                    c,
                    "work",
                    &work.id,
                    &work.project_key,
                    work.created_at,
                    &work,
                )?;
                emit(c, "work", &work)?;
            }
            run.provider_id = run.provider_id.map(|id| remote_provider_id(host_id, &id));
            run.host_id = host_id.into();
            run.observation_source = format!("host:{host_id}");
            put(
                c,
                "setting",
                &format!("remote_owned:{}", run.id),
                "",
                now(),
                &true,
            )?;
            save_run(c, &run)?;
            Ok(true)
        })
    }
    pub fn import_external(&self, run: Run, messages: Vec<Message>) -> Result<Run> {
        if run.origin != Origin::External {
            return Err(Error::Invalid("외부 실행만 등록할 수 있습니다.".into()));
        }
        self.write(|c| {
            if let Some(existing) = get::<Run>(c, "run", &run.id)?
                && existing.origin != Origin::External
            {
                return Err(Error::Conflict(
                    "관리 중인 실행을 외부 자료로 덮어쓸 수 없습니다.".into(),
                ));
            }
            let _: Project = required(c, "project", &run.project_key)?;
            if get::<Work>(c, "work", &run.work_id)?.is_none() {
                let w = Work {
                    id: run.work_id.clone(),
                    project_key: run.project_key.clone(),
                    conversation_id: run.conversation_id.clone(),
                    title: run.title.clone(),
                    goal: run.context.goal.clone(),
                    context_revision: run.context_revision,
                    constraints: run.context.constraints.clone(),
                    decisions: vec![],
                    performed_actions: vec![],
                    open_questions: vec![],
                    references: vec![],
                    created_at: run.created_at,
                };
                put(c, "work", &w.id, &w.project_key, w.created_at, &w)?;
                emit(c, "work", &w)?;
            }
            save_run(c, &run)?;
            for m in messages {
                if m.run_id != run.id {
                    return Err(Error::Invalid(
                        "외부 메시지 대상이 일치하지 않습니다.".into(),
                    ));
                }
                put(c, "message", &m.id, &run.id, m.created_at, &m)?;
            }
            Ok(run)
        })
    }
    pub fn inbox(&self, conversation: Option<&str>) -> Result<Vec<InboxEntry>> {
        self.read(|c| list(c, "inbox", conversation))
    }
    pub fn inbox_entry(&self, id: &str) -> Result<InboxEntry> {
        self.read(|c| required(c, "inbox", id))
    }
    pub fn detail(&self, id: &str) -> Result<RunDetail> {
        self.read(|c| {
            let run: Run = required(c, "run", id)?;
            Ok(RunDetail {
                conversation: conversation_messages(c, &run)?,
                children: descendants(c, &run)?
                    .into_iter()
                    .map(|run| {
                        Ok(AgentDetail {
                            messages: list(c, "message", Some(&run.id))?,
                            run,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                messages: list(c, "message", Some(id))?,
                inbox: list(c, "inbox", Some(&run.conversation_id))?,
                approvals: list(c, "approval", Some(id))?,
                inputs: list(c, "input", Some(id))?,
                run,
            })
        })
    }
    pub fn events(&self, after: i64, limit: usize) -> Result<Vec<Event>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT seq,id,kind,created_at,data FROM events WHERE seq>?1 ORDER BY seq LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![after, limit.clamp(1, 1000)], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            rows.map(|v| {
                let (seq, id, kind, created_at, data) = v?;
                Ok(Event {
                    seq,
                    id,
                    kind,
                    created_at,
                    data: serde_json::from_str(&data)?,
                })
            })
            .collect()
        })
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.read(|c| {
            let mut inbox: Vec<InboxEntry> = list(c, "inbox", None)?;
            for entry in &mut inbox {
                entry.result = entry.result.chars().take(512).collect();
            }
            let hidden: Vec<String> = list(c, "hidden_session", None)?;
            let (removed_sessions, runs): (Vec<Run>, Vec<Run>) = list::<Run>(c, "run", None)?
                .into_iter()
                .partition(|r| hidden.iter().any(|s| s == r.session_id()));
            Ok(Snapshot {
                server_id: required(c, "setting", "server_id")?,
                version: VERSION.into(),
                last_seq: c
                    .query_row("SELECT coalesce(max(seq),0) FROM events", [], |r| r.get(0))?,
                projects: list(c, "project", None)?,
                works: list(c, "work", None)?,
                runs,
                removed_sessions,
                providers: preferences::all_connections(c)?,
                model_history: list(c, "model_history", None)?,
                model_selection: get(c, "setting", "model_selection")?,
                transmissions: list(c, "transmission", None)?,
                inbox,
                hosts: list(c, "host", None)?,
                quotas: list(c, "quota", None)?,
                approvals: list(c, "approval", None)?,
            })
        })
    }
}
