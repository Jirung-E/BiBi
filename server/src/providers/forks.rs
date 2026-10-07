//! A fork copies a fixed conversation prefix, never resumes or mutates its source.
mod claude;
use super::{claude_live, rpc::Rpc};
use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Serialize)]
struct Point {
    id: String,
    revision: String,
    excerpt: String,
    created_at: i64,
    #[serde(skip)]
    end: usize,
}
fn revisions(values: &[Value]) -> Result<Vec<String>> {
    let mut hash = Sha256::new();
    values
        .iter()
        .map(|value| {
            hash.update(serde_json::to_vec(value)?);
            hash.update(b"\n");
            Ok(format!("{:x}", hash.clone().finalize()))
        })
        .collect()
}
fn point(id: String, revision: &str, excerpt: &str, created_at: i64, end: usize) -> Point {
    Point {
        id,
        revision: revision.into(),
        excerpt: excerpt.chars().take(160).collect(),
        created_at,
        end,
    }
}
fn candidate(engine: &Engine, id: &str) -> Result<(Engine, Run)> {
    let run = engine.store.run(id)?;
    if run.host_id != "local" {
        bail!("포크는 이 세션이 저장된 호스트에서 실행하세요.");
    }
    if run.agent_kind == "subagent" {
        bail!("서브에이전트는 부모 대화에서 포크하세요.");
    }
    if !matches!(
        run.provider,
        Provider::Claude | Provider::Codex | Provider::Ollama | Provider::OpenAi | Provider::Mock
    ) {
        bail!("이 제공자는 대화 포크를 지원하지 않습니다.");
    }
    if engine
        .store
        .snapshot()?
        .removed_sessions
        .iter()
        .any(|r| r.session_id() == run.session_id())
    {
        bail!("제거한 세션은 복원한 뒤 포크하세요.");
    }
    if !claude_live::same_workspace(
        &engine.store.project(&run.project_key)?.workspace,
        &run.workspace,
    ) {
        bail!("프로젝트 작업 폴더가 변경되었습니다. 원래 프로젝트에서 포크하세요.");
    }
    let configured = if let Some(id) = &run.provider_id {
        engine.configured(id)?
    } else {
        engine.clone()
    };
    if configured
        .provider
        .as_ref()
        .is_some_and(|p| p.adapter != run.provider)
    {
        bail!("제공자 연결 방식이 변경되었습니다.");
    }
    if run.provider == Provider::Claude {
        for arg in &configured.config.claude_args {
            let flag = arg.split('=').next().unwrap_or(arg);
            if matches!(
                flag,
                "--fork-session"
                    | "--continue"
                    | "-c"
                    | "--resume"
                    | "-r"
                    | "--resume-session-at"
                    | "--session-id"
                    | "--background"
                    | "--bg"
                    | "--no-session-persistence"
            ) {
                bail!("제공자 실행 인수의 {flag}가 포크 대화 이어가기와 충돌합니다.");
            }
        }
    }
    Ok((configured, run))
}
enum Content {
    Claude(claude::Transcript),
    Codex { rpc: Box<Rpc>, turns: Vec<Value> },
    Stored(Vec<Message>),
}
struct Prepared {
    run: Run,
    points: Vec<Point>,
    content: Content,
}
async fn prepare(engine: &Engine, id: &str) -> Result<Prepared> {
    let (engine, run) = candidate(engine, id)?;
    let (points, content) = match run.provider {
        Provider::Claude => {
            let source = run.clone();
            let store = engine.store.clone();
            let transcript =
                tokio::task::spawn_blocking(move || claude::load(&store, &source)).await??;
            let points = transcript.points()?;
            (points, Content::Claude(transcript))
        }
        Provider::Codex => {
            let native = run
                .session_key
                .as_deref()
                .context("원본 Codex 세션 ID가 없습니다.")?;
            let mut rpc = Rpc::connect_project(&engine.config, &run.workspace).await?;
            let data = rpc
                .request(
                    "thread/read",
                    json!({"threadId":native,"includeTurns":false}),
                )
                .await?;
            if data["thread"]["id"] != native
                || !data["thread"]["cwd"]
                    .as_str()
                    .is_some_and(|cwd| claude_live::same_workspace(cwd, &run.workspace))
            {
                bail!("Codex 원본 ID 또는 작업 폴더가 일치하지 않습니다.");
            }
            let mut turns = vec![];
            let mut cursor = Value::Null;
            for _ in 0..40 {
                let page=rpc.request("thread/turns/list",json!({"threadId":native,"limit":50,"cursor":cursor,"sortDirection":"desc","itemsView":"full"})).await?;
                turns.extend(
                    page["data"]
                        .as_array()
                        .context("Codex 턴 목록 형식 오류")?
                        .iter()
                        .cloned(),
                );
                cursor = page["nextCursor"].clone();
                if cursor.is_null() {
                    break;
                }
            }
            if !cursor.is_null() {
                bail!("포크 이력이 2,000턴 조회 범위를 초과했습니다.");
            }
            turns.reverse();
            let hashes = revisions(&turns)?;
            let mut points = vec![];
            for (i, turn) in turns.iter().enumerate() {
                if turn["status"] != "completed" {
                    continue;
                }
                let messages = codex_messages(std::slice::from_ref(turn));
                if let Some(last) = messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "assistant" && m.phase.as_deref() != Some("commentary"))
                {
                    points.push(point(
                        turn["id"].as_str().context("Codex 턴 ID 없음")?.into(),
                        &hashes[i],
                        &last.text,
                        last.created_at,
                        i,
                    ));
                }
            }
            (
                points,
                Content::Codex {
                    rpc: Box::new(rpc),
                    turns,
                },
            )
        }
        _ => {
            let messages = engine.store.detail(&run.id)?.conversation;
            let values = messages
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let hashes = revisions(&values)?;
            let mut points = vec![];
            for (i, m) in messages.iter().enumerate() {
                if m.role != "assistant"
                    || m.phase.as_deref() == Some("commentary")
                    || m.text.trim().is_empty()
                {
                    continue;
                }
                let owner = engine.store.run(&m.run_id)?;
                if !owner.state.terminal() || owner.state == RunState::Uncertain {
                    continue;
                }
                // Stored Chat API payloads are committed at complete turn boundaries.
                if run.provider == Provider::Ollama
                    && messages
                        .iter()
                        .skip(i + 1)
                        .any(|next| next.run_id == m.run_id)
                {
                    continue;
                }
                points.push(point(m.id.clone(), &hashes[i], &m.text, m.created_at, i));
            }
            (points, Content::Stored(messages))
        }
    };
    Ok(Prepared {
        run,
        points,
        content,
    })
}
pub async fn points(engine: &Engine, id: &str) -> Result<Value> {
    let source = prepare(engine, id).await?;
    Ok(
        json!({"points":source.points,"workspace":source.run.workspace,"mode":if matches!(source.run.provider,Provider::Codex|Provider::Claude){"native"}else{"history"}}),
    )
}
pub async fn fork(engine: &Engine, request: &ForkRequest) -> Result<Run> {
    let _guard = engine.fork_lock.lock().await;
    if let Some(run) = engine.store.fork_request(request)? {
        return Ok(run);
    }
    let mut source = prepare(engine, &request.run_id).await?;
    let selected = source
        .points
        .iter()
        .find(|p| p.id == request.point_id)
        .context("선택한 분기 지점이 없습니다. 다시 열어 확인하세요.")?;
    if selected.revision != request.revision {
        bail!("분기 지점의 이력이 변경되었습니다. 다시 열어 확인하세요.");
    }
    let end = selected.end;
    engine.store.begin_fork(request)?;
    let (native, path, messages, history, root) = match &mut source.content {
        Content::Claude(transcript) => {
            let fork = transcript.fork(end, &request.title)?;
            (
                fork.native,
                Some(fork.path),
                transcript.messages(end),
                None,
                Some(transcript.root.to_string_lossy().into_owned()),
            )
        }
        Content::Codex { rpc, turns } => {
            let original = source
                .run
                .session_key
                .as_deref()
                .context("Codex 세션 ID 없음")?;
            let response=rpc.request("thread/fork",json!({"threadId":original,"lastTurnId":request.point_id,"cwd":source.run.workspace,"ephemeral":false})).await?;
            let thread = &response["thread"];
            let native = thread["id"].as_str().context("Codex 포크 ID 없음")?;
            if native == original
                || thread["forkedFromId"]
                    .as_str()
                    .is_some_and(|id| id != original)
            {
                bail!("Codex가 독립된 포크 세션을 반환하지 않았습니다.");
            }
            // A provider that ignores lastTurnId must not silently copy later turns.
            let copied = rpc
                .request(
                    "thread/read",
                    json!({"threadId":native,"includeTurns":true}),
                )
                .await?;
            let copied_turns = copied["thread"]["turns"]
                .as_array()
                .context("Codex 포크 이력 확인 실패")?;
            if copied_turns.last().and_then(|t| t["id"].as_str()) != Some(&request.point_id) {
                bail!("Codex가 선택한 분기 지점을 보존하지 않았습니다. CLI를 업데이트하세요.");
            }
            (
                native.into(),
                thread["path"].as_str().map(String::from),
                codex_messages(&turns[..=end]),
                None,
                None,
            )
        }
        Content::Stored(messages) => {
            let history = if source.run.provider == Provider::Ollama {
                engine
                    .store
                    .setting::<Value>(&format!("ollama:history:{}", messages[end].run_id))?
            } else {
                None
            };
            (
                uuid::Uuid::new_v4().to_string(),
                None,
                messages[..=end].to_vec(),
                history,
                None,
            )
        }
    };
    Ok(engine.store.finish_fork(
        request,
        &source.run,
        ForkContent {
            native,
            session_file: path,
            messages,
            history,
            claude_root: root,
        },
    )?)
}
fn codex_messages(turns: &[Value]) -> Vec<Message> {
    let mut messages = vec![];
    for turn in turns {
        for item in turn["items"].as_array().into_iter().flatten() {
            let (role, text) = match item["type"].as_str() {
                Some("userMessage") => (
                    "user",
                    item["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                Some("agentMessage") => ("assistant", item["text"].as_str().unwrap_or("").into()),
                Some("commandExecution") => (
                    "tool",
                    item["aggregatedOutput"].as_str().unwrap_or("").into(),
                ),
                _ => continue,
            };
            if text.is_empty() {
                continue;
            }
            messages.push(Message {
                id: item["id"].as_str().unwrap_or("").into(),
                run_id: String::new(),
                role: role.into(),
                text,
                phase: item["phase"].as_str().map(String::from),
                created_at: turn["startedAt"].as_i64().unwrap_or(0) * 1000,
            });
        }
    }
    messages
}
