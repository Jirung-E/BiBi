use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
#[derive(Deserialize)]
pub struct Upload {
    pub project_key: String,
    pub provider_id: String,
    pub document: String,
}
pub fn key(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
pub fn result(project: &Project, imported: usize, errors: Vec<Value>) -> Value {
    json!({"imported":imported,"errors":errors,"scope":{"workspace":project.workspace,"host":"local","max_sessions":2000,"active_control":false}})
}
pub fn run(
    engine: &Engine,
    project: &Project,
    identity: &str,
    title: &str,
    model: &str,
    created: i64,
) -> Result<Run> {
    let provider = engine.provider.as_ref().context("제공자 설정 없음")?;
    let id = format!("external_{}", key(&[&project.id, &provider.id, identity]));
    let work = format!("work_{id}");
    let conversation = format!("conversation_{id}");
    Ok(Run {
        id: id.clone(),
        session_id: id.clone(),
        continued_from: None,
        parent_session_id: None,
        agent_kind: "session".into(),
        project_key: project.id.clone(),
        work_id: work.clone(),
        conversation_id: conversation.clone(),
        request_id: format!("request_{id}"),
        parent_run_id: None,
        context_revision: 1,
        role: "외부 실행".into(),
        title: title.chars().take(160).collect(),
        provider: provider.adapter.clone(),
        provider_id: Some(provider.id.clone()),
        model: model.into(),
        host_id: "local".into(),
        state: RunState::Disconnected,
        phase: "저장된 이력 조회".into(),
        wait_reason: None,
        observation_source: "history/import".into(),
        created_at: created,
        updated_at: created,
        observed_at: now(),
        session_key: Some(identity.into()),
        turn_id: None,
        origin: Origin::External,
        workspace: project.workspace.clone(),
        read_only: false,
        approval_mode: ApprovalMode::OnRequest,
        capabilities: Capabilities::external(&provider.adapter),
        context: ContextPacket {
            attachments: vec![],
            schema_version: 1,
            project_key: project.id.clone(),
            work_id: work,
            conversation_id: conversation.clone(),
            request_id: format!("request_{id}"),
            parent_request_id: None,
            reply_to_response_id: None,
            context_revision: 1,
            role: "외부 실행".into(),
            question: title.into(),
            goal: title.into(),
            constraints: project.constraints.clone(),
            decisions: vec![],
            performed_actions: vec![],
            open_questions: vec![],
            references: vec![],
            source_runs: vec![],
            previous_answer_excerpt: None,
            excerpt_truncated: false,
            workspace: project.workspace.clone(),
            reply_to: format!("bibi://inbox/{conversation}"),
        },
        stats: UsageStats::default(),
        runtime: RuntimeMetadata {
            provider_name: Some(provider.name.clone()),
            ..Default::default()
        },
        activity: None,
        error: None,
    })
}

pub fn upload(engine: &Engine, upload: Upload) -> Result<Value> {
    if upload.document.len() > MAX_FILE_BYTES {
        bail!("대화 파일은 8 MiB 이하만 가져올 수 있습니다.");
    }
    let engine = engine.configured(&upload.provider_id)?;
    let provider = engine.provider.as_ref().context("제공자 설정 없음")?;
    if provider.host_id != "local"
        || !matches!(
            provider.adapter,
            Provider::Ollama | Provider::OpenAi | Provider::Command
        )
    {
        bail!("이 제공자는 파일 가져오기 대신 저장된 세션 조회를 사용하세요.");
    }
    let project = engine.store.project(&upload.project_key)?;
    let document: Value =
        serde_json::from_str(&upload.document).context("올바른 JSON 대화 파일이 아닙니다.")?;
    let source = document
        .as_array()
        .or_else(|| document["messages"].as_array())
        .context(
            "messages 배열 또는 메시지 배열이 필요합니다. 각 메시지에 role과 content를 넣으세요.",
        )?;
    if source.is_empty() || source.len() > 10000 {
        bail!("대화 파일에는 1~10,000개 메시지가 필요합니다.");
    }
    let mut transcript = Vec::new();
    for message in source {
        let role = message["role"]
            .as_str()
            .context("메시지 role이 없습니다.")?;
        if !matches!(role, "user" | "assistant" | "system" | "tool") {
            bail!("지원하지 않는 메시지 role: {role}");
        }
        let content = message["content"].as_str().context("content는 텍스트여야 합니다. 이미지·도구 호출이 포함된 원본은 텍스트 대화로 내보내세요.")?;
        if message
            .get("tool_calls")
            .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|v| !v.is_empty()))
        {
            bail!("도구 호출이 포함된 파일입니다. 도구 기록을 텍스트로 내보낸 뒤 가져오세요.");
        }
        transcript.push((role.to_owned(), content.to_owned()));
    }
    if !transcript.iter().any(|(role, text)| {
        matches!(role.as_str(), "user" | "assistant") && !text.trim().is_empty()
    }) {
        bail!("사용자 또는 모델의 대화 내용이 필요합니다.");
    }
    let title = document["title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            transcript
                .iter()
                .find(|(role, _)| role == "user")
                .map(|(_, text)| text.as_str())
        })
        .unwrap_or("가져온 대화");
    let model = document["model"].as_str().unwrap_or("");
    if model.len() > 256 {
        bail!("모델 이름은 256자 이하여야 합니다.");
    }
    let identity = key(&[&serde_json::to_string(&transcript)?, model]);
    let mut run = run(
        &engine,
        &project,
        &identity,
        title,
        model,
        now() - transcript.len() as i64,
    )?;
    run.id = format!("imported_{}", run.id);
    run.session_id = run.id.clone();
    run.session_key = Some(format!("transcript_{identity}"));
    run.origin = Origin::Managed;
    run.role = "업무 조정".into();
    run.context.role = run.role.clone();
    run.state = RunState::Completed;
    run.phase = "대화 파일 가져옴".into();
    run.read_only = provider.adapter == Provider::Ollama;
    run.capabilities = Capabilities::managed(&provider.adapter);
    run.context.references.push(Evidence {
        text: "사용자가 가져온 텍스트 대화".into(),
        source: "file:conversation.json".into(),
        revision: identity,
    });
    run.context.previous_answer_excerpt = transcript
        .iter()
        .rev()
        .find(|(role, _)| role == "assistant")
        .map(|(_, text)| text.chars().take(6000).collect());
    run.context.excerpt_truncated = transcript
        .iter()
        .rev()
        .find(|(role, _)| role == "assistant")
        .is_some_and(|(_, text)| text.chars().count() > 6000);
    let messages = transcript
        .into_iter()
        .enumerate()
        .map(|(i, (role, text))| Message {
            attachments: vec![],
            id: format!("{}:{i}", run.id),
            run_id: run.id.clone(),
            role,
            text,
            created_at: run.created_at + i as i64,
            phase: None,
        })
        .collect();
    engine.store.import_conversation(run, messages)?;
    Ok(result(&project, 1, vec![]))
}
