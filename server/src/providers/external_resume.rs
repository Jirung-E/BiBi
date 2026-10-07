//! Explicit transfer of a saved native conversation to BiBi. Probing never
//! starts a model turn; running external terminals are not input bridges.
use super::{claude_history, claude_live, codex, rpc::Rpc};
use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::{Value, json};

struct Probe {
    run: Run,
    messages: Vec<Message>,
    source: Value,
    confirmed_exit: bool,
}
fn candidate(engine: &Engine, id: &str) -> Result<(Engine, Run)> {
    let run = engine.store.run(id)?;
    if run.origin != Origin::External || run.host_id != "local" {
        bail!("이 호스트의 외부 세션에서 이어받기를 실행하세요.");
    }
    if !matches!(run.provider, Provider::Claude | Provider::Codex)
        || run.agent_kind == "subagent"
        || run.parent_session_id.is_some()
    {
        bail!("이 세션은 독립 복원을 지원하지 않습니다. 부모 대화에서 이어가세요.");
    }
    let native = run
        .session_key
        .as_deref()
        .context("원본 세션 ID가 없습니다.")?;
    uuid::Uuid::parse_str(native).context("잘못된 원본 세션 ID입니다.")?;
    let snapshot = engine.store.snapshot()?;
    if snapshot
        .removed_sessions
        .iter()
        .any(|r| r.session_id() == run.session_id())
    {
        bail!("제거한 세션은 복원한 뒤 이어받으세요.");
    }
    if snapshot
        .runs
        .iter()
        .chain(&snapshot.removed_sessions)
        .any(|r| {
            r.origin == Origin::Managed
                && r.host_id == run.host_id
                && r.provider == run.provider
                && r.session_key == run.session_key
        })
    {
        bail!("같은 원본 대화가 이미 BiBi에 연결되어 있습니다. 해당 세션을 여세요.");
    }
    let engine = engine.configured(
        run.provider_id
            .as_deref()
            .context("원래 제공자 연결이 없습니다. 다시 가져오세요.")?,
    )?;
    if engine
        .provider
        .as_ref()
        .is_none_or(|p| p.adapter != run.provider)
    {
        bail!("원래 제공자 연결 방식이 변경되었습니다.");
    }
    compatible_arguments(&engine, &run)?;
    if !claude_live::same_workspace(
        &engine.store.project(&run.project_key)?.workspace,
        &run.workspace,
    ) {
        bail!("프로젝트의 작업 폴더가 변경되었습니다. 원래 프로젝트에서 이어받으세요.");
    }
    Ok((engine, run))
}
fn compatible_arguments(engine: &Engine, run: &Run) -> Result<()> {
    if run.provider == Provider::Claude {
        for arg in &engine.config.claude_args {
            let flag = arg.split('=').next().unwrap_or(arg);
            if matches!(
                flag,
                "--fork-session"
                    | "--continue"
                    | "-c"
                    | "--resume"
                    | "-r"
                    | "--session-id"
                    | "--background"
                    | "--bg"
            ) {
                bail!("제공자 실행 인수의 {flag}가 이어받기와 충돌합니다. 해당 인수를 제거하세요.");
            }
        }
    }
    Ok(())
}
fn claude_owner(root: &std::path::Path, native: &str) -> Result<bool> {
    let registrations = claude_live::registrations(root)?;
    match registrations.get(native) {
        Some(live) if live.state == RunState::Disconnected => Ok(true),
        Some(live) if live.verified => {
            bail!("원래 Claude 세션이 아직 실행 중입니다. 원래 앱에서 종료한 뒤 다시 확인하세요.")
        }
        Some(_) => bail!(
            "원래 Claude 프로세스의 종료를 확인할 수 없습니다. 원래 앱을 종료하고 실행 등록 상태를 확인하세요."
        ),
        None => Ok(false),
    }
}
async fn codex_owner(rpc: &mut Rpc, run: &Run) -> Result<()> {
    let native = run
        .session_key
        .as_deref()
        .context("Codex 세션 ID가 없습니다.")?;
    let value = rpc
        .request(
            "thread/read",
            json!({"threadId":native,"includeTurns":false}),
        )
        .await?;
    let thread = &value["thread"];
    if thread["id"].as_str() != Some(native) {
        bail!("Codex가 다른 원본 대화를 반환했습니다. 이어받기를 중단했습니다.");
    }
    let cwd = thread["cwd"]
        .as_str()
        .context("Codex 원본 작업 폴더를 확인할 수 없습니다.")?;
    if !claude_live::same_workspace(cwd, &run.workspace) {
        bail!("Codex 원본 대화의 작업 폴더가 다릅니다.");
    }
    if thread["status"]["type"] == "active" {
        bail!("원래 Codex 세션이 아직 실행 중입니다. 원래 앱에서 종료한 뒤 다시 확인하세요.");
    }
    Ok(())
}
async fn probe(engine: &Engine, id: &str) -> Result<Probe> {
    let (engine, mut run) = candidate(engine, id)?;
    let native = run.session_key.clone().unwrap();
    let mut source = json!({"native":native,"provider_id":run.provider_id,"pending":true});
    let (confirmed_exit, messages) = match run.provider {
        Provider::Claude => {
            let observed = run.clone();
            let (root, messages, stopped) = tokio::task::spawn_blocking(move || -> Result<_> {
                let (root, messages) = claude_history::resume_source(&observed)?;
                let stopped = claude_owner(&root, observed.session_key.as_deref().unwrap())?;
                Ok((root, messages, stopped))
            })
            .await??;
            source["claude_root"] = json!(root);
            (stopped, messages)
        }
        Provider::Codex => {
            let mut rpc = Rpc::connect_project(&engine.config, &run.workspace).await?;
            codex_owner(&mut rpc, &run).await?;
            // Refresh saved public history before the ownership transaction.
            codex::import(
                &engine,
                &engine.store.project(&run.project_key)?,
                &mut rpc,
                &native,
            )
            .await?;
            let _ = rpc.stop().await;
            run = engine.store.run(id)?;
            if run.state == RunState::Running {
                bail!("원래 Codex 세션이 다시 실행 중입니다. 종료한 뒤 이어받으세요.");
            }
            let messages = engine.store.detail(id)?.messages;
            if messages.is_empty() {
                bail!("복원할 Codex 원본 대화가 없습니다.");
            }
            // An isolated app-server's unloaded/idle status does not prove that
            // the official app or another terminal has stopped using this ID.
            (false, messages)
        }
        _ => unreachable!(),
    };
    Ok(Probe {
        run,
        messages,
        source,
        confirmed_exit,
    })
}
pub async fn check(engine: &Engine, id: &str) -> Result<Value> {
    let probe = probe(engine, id).await?;
    Ok(json!({"native_id":probe.run.session_key,"requires_confirmation":!probe.confirmed_exit}))
}
pub async fn adopt(
    engine: &Engine,
    id: &str,
    expected_native: &str,
    confirmed_stopped: bool,
) -> Result<Run> {
    let existing = engine.store.run(id)?;
    // Retrying after a lost response is safe and never opens another session.
    if existing.origin == Origin::Managed
        && existing.observation_source == "bibi/external-resume"
        && existing.session_key.as_deref() == Some(expected_native)
    {
        return Ok(existing);
    }
    if existing.session_key.as_deref() != Some(expected_native) {
        bail!("이어받을 원본 세션이 변경되었습니다. 다시 확인하세요.");
    }
    let probe = probe(engine, id).await?;
    if !probe.confirmed_exit && !confirmed_stopped {
        bail!("원래 앱에서 이 대화를 종료했는지 확인한 뒤 이어받으세요.");
    }
    engine
        .store
        .adopt_external(&probe.run, probe.messages, probe.source)
        .map_err(Into::into)
}
/// Check again when the first queued request actually starts, so a process
/// opened after the UI probe cannot be mistaken for an ended conversation.
pub(crate) async fn before_start(engine: &Engine, run: &Run) -> Result<()> {
    let key = format!("external_resume:{}", run.session_id());
    let Some(source) = engine.store.setting::<Value>(&key)? else {
        return Ok(());
    };
    compatible_arguments(engine, run)?;
    if source["pending"] != true {
        return Ok(());
    }
    if source["native"].as_str() != run.session_key.as_deref()
        || source["provider_id"].as_str() != run.provider_id.as_deref()
    {
        bail!("이어받기 대상이 변경되었습니다. 원래 대화 ID를 보존했습니다.");
    }
    match run.provider {
        Provider::Claude => {
            let observed = run.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                let (root, _) = claude_history::resume_source(&observed)?;
                claude_owner(&root, observed.session_key.as_deref().unwrap())?;
                Ok(())
            })
            .await??;
        }
        Provider::Codex => {
            let mut rpc = Rpc::connect_project(&engine.config, &run.workspace).await?;
            let result = codex_owner(&mut rpc, run).await;
            let _ = rpc.stop().await;
            result?;
        }
        _ => bail!("이 제공자는 외부 세션 복원을 지원하지 않습니다."),
    }
    Ok(())
}
