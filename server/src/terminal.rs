//! Local native launch tickets. No TCP listener, shell prompt injection or model request.
mod platform;
use crate::{
    catalog::{self, Entry},
    providers::{claude_live, launch, session_window},
    runtime::Engine,
};
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct Ticket {
    provider: Provider,
    native: String,
    root: PathBuf,
    file: PathBuf,
    workspace: String,
    provider_id: String,
    provider_config: Value,
    args: Vec<String>,
    expires: i64,
}
fn owner(entry: &Entry) -> Result<Option<claude_live::LiveSession>> {
    if entry.provider != Provider::Claude {
        return Ok(None);
    }
    let live =
        claude_live::registrations(&entry.root)?.remove(entry.native_id.as_deref().unwrap_or(""));
    if live
        .as_ref()
        .is_some_and(|l| !claude_live::same_workspace(&l.registration.cwd, &entry.workspace))
    {
        bail!("원래 프로세스와 작업 폴더가 일치하지 않습니다.");
    }
    Ok(live)
}
pub async fn check(engine: &Engine, id: &str) -> Result<Value> {
    let e = engine.clone();
    let id = id.to_owned();
    tokio::task::spawn_blocking(move||{
        let entry=catalog::find(&e,&id)?;
        if !entry.terminal_supported {bail!("{}",entry.terminal_reason.as_deref().unwrap_or("터미널 재개 미지원"));}
        catalog::verify(&entry)?;
        let managed=entry.run_id.as_deref().map(|id|e.store.run(id)).transpose()?.is_some_and(|r|r.origin==Origin::Managed);
        let live=owner(&entry)?;
        let released=entry.run_id.as_deref().map(|id|e.store.run(id)).transpose()?.map(|r|e.store.setting::<String>(&format!("native_release:{}",r.session_id()))).transpose()?.flatten().as_deref()==Some("released");
        let focus=(!managed||released)&&live.as_ref().is_some_and(|l|l.verified);
        if !managed&&live.as_ref().is_some_and(|l|!l.verified&&l.state!=RunState::Disconnected){bail!("원래 프로세스의 종료를 확인하지 못했습니다. 실행 등록 상태를 확인하세요.");}
        let confirm=!managed&&!focus&&live.is_none() || entry.provider==Provider::Codex;
        Ok(json!({"entry":entry,"action":if focus{"focus"}else{"resume"},"requires_confirmation":confirm}))
    }).await?
}
fn launch_args(
    provider: &Provider,
    configured: &[String],
    run: Option<&Run>,
    native: &str,
) -> Result<Vec<String>> {
    // Preserve supported user options, but never interpret a positional value as a prompt.
    let value_flags: &[&str] = if *provider == Provider::Claude {
        &[
            "--model",
            "--agent",
            "--settings",
            "--setting-sources",
            "--add-dir",
            "--mcp-config",
            "--plugin-dir",
            "--permission-mode",
            "--allowedTools",
            "--disallowedTools",
            "--allowed-tools",
            "--disallowed-tools",
            "--tools",
            "--system-prompt",
            "--append-system-prompt",
            "--effort",
        ]
    } else {
        &[
            "-c",
            "--config",
            "-p",
            "--profile",
            "--enable",
            "--disable",
            "-m",
            "--model",
            "--local-provider",
            "-s",
            "--sandbox",
            "-a",
            "--ask-for-approval",
            "--add-dir",
        ]
    };
    let switches: &[&str] = if *provider == Provider::Claude {
        &[
            "--strict-mcp-config",
            "--allow-dangerously-skip-permissions",
        ]
    } else {
        &["--no-alt-screen", "--search", "--oss", "--strict-config"]
    };
    let mut i = 0;
    while i < configured.len() {
        let a = &configured[i];
        let flag = a.split('=').next().unwrap();
        if value_flags.contains(&flag) {
            if !a.contains('=') {
                i += 1;
                if i == configured.len() || configured[i].starts_with('-') {
                    bail!("실행 인수 {flag}의 값이 없습니다.");
                }
            }
        } else if !switches.contains(&a.as_str()) {
            bail!("터미널 재개와 함께 사용할 수 없는 제공자 인수입니다: {flag}");
        }
        i += 1;
    }
    let mut args = Vec::new();
    let mut index = 0;
    while index < configured.len() {
        let flag = configured[index].split('=').next().unwrap();
        let overridden = run.is_some()
            && if *provider == Provider::Claude {
                flag == "--permission-mode"
            } else {
                matches!(flag, "--sandbox" | "-s" | "--ask-for-approval" | "-a")
            };
        if overridden {
            if !configured[index].contains('=') {
                index += 1;
            }
        } else {
            args.push(configured[index].clone());
        }
        index += 1;
    }
    if *provider == Provider::Claude {
        if let Some(r) = run {
            args.extend([
                "--permission-mode".into(),
                if r.read_only {
                    "plan"
                } else {
                    match r.approval_mode {
                        ApprovalMode::OnRequest => "default",
                        ApprovalMode::AcceptEdits => "acceptEdits",
                        ApprovalMode::FullAccess => "bypassPermissions",
                    }
                }
                .into(),
            ]);
        }
        args.extend(["--resume".into(), native.into()]);
    } else {
        if let Some(r) = run {
            args.extend([
                "--sandbox".into(),
                if r.read_only {
                    "read-only"
                } else if r.approval_mode == ApprovalMode::FullAccess {
                    "danger-full-access"
                } else {
                    "workspace-write"
                }
                .into(),
            ]);
        }
        if let Some(r) = run {
            args.extend([
                "--ask-for-approval".into(),
                if r.approval_mode == ApprovalMode::FullAccess && !r.read_only {
                    "never"
                } else {
                    "on-request"
                }
                .into(),
            ]);
        }
        args.extend(["resume".into(), native.into()]);
    }
    Ok(args)
}
pub async fn prepare(engine: &Engine, id: &str, confirmed: bool) -> Result<Value> {
    let checked = check(engine, id).await?;
    let entry = catalog::find(engine, id)?;
    if checked["action"] == "focus" {
        let live = owner(&entry)?.context("원래 프로세스가 종료됐습니다. 다시 확인하세요.")?;
        if !live.verified {
            bail!("프로세스 상태가 바뀌었습니다.");
        }
        return tokio::task::spawn_blocking(move || session_window::focus(&live.registration))
            .await?;
    }
    if checked["requires_confirmation"] == true && !confirmed {
        bail!("원래 앱에서 같은 세션을 종료했는지 확인해 주세요.");
    }
    let native = entry.native_id.clone().context("원본 ID 없음")?;
    let provider = engine
        .store
        .provider(entry.provider_id.as_deref().context("원래 제공자 없음")?)?;
    if provider.adapter != entry.provider || provider.host_id != "local" {
        bail!("제공자 연결이 변경됐습니다.");
    }
    let run = entry
        .run_id
        .as_deref()
        .map(|id| engine.store.run(id))
        .transpose()?;
    let args = launch_args(
        &provider.adapter,
        &provider.args,
        run.as_ref().filter(|r| r.origin == Origin::Managed),
        &native,
    )?;
    launch::describe(&provider.command)?;
    let ticket = uuid::Uuid::new_v4().to_string();
    engine
        .store
        .claim_terminal(&entry.provider, &native, &ticket)?;
    let result=async {
        if let Some(run)=&run && run.origin==Origin::Managed {
            if entry.provider==Provider::Claude {engine.release_claude(&run.id,&native).await?;}
            else {
                engine.release_codex(&run.id,&native,&entry.root.to_string_lossy()).await?;
            }
        }
        if let Some(live)=owner(&entry)? && live.state!=RunState::Disconnected {bail!("같은 세션이 원래 CLI에서 열려 있습니다. 원래 창을 사용하세요.");}
        catalog::verify(&entry)?;
        let data=Ticket{provider:entry.provider.clone(),native:native.clone(),root:entry.root.clone(),file:entry.file.clone(),workspace:entry.workspace.clone(),provider_id:provider.id.clone(),provider_config:serde_json::to_value(&provider)?,args,expires:now()+60_000};
        engine.store.save_terminal_ticket(&ticket,&serde_json::to_value(data)?)?;
        Ok(json!({"status":"prepared","ticket":ticket,"workspace":entry.workspace,"native_id":native}))
    }.await;
    if result.is_err() {
        let _ = engine
            .store
            .renew_terminal(&entry.provider, &native, &ticket, true);
    }
    result
}
pub async fn open(engine: &Engine, id: &str, confirmed: bool) -> Result<Value> {
    let result = prepare(engine, id, confirmed).await?;
    if let Some(ticket) = result["ticket"].as_str() {
        let data = engine.config.data_dir.clone();
        let key = ticket.to_string();
        if let Err(e) = tokio::task::spawn_blocking(move || platform::open(&data, &key)).await? {
            cancel_ticket(&engine.store, ticket);
            return Err(e);
        }
        Ok(
            json!({"status":"requested","message":"이 호스트의 터미널에 같은 세션 열기를 요청했습니다."}),
        )
    } else {
        Ok(result)
    }
}
fn cancel_ticket(store: &Store, ticket: &str) {
    if let Ok(v) = store.take_terminal_ticket(ticket)
        && let Ok(t) = serde_json::from_value::<Ticket>(v)
    {
        let _ = store.renew_terminal(&t.provider, &t.native, ticket, true);
    }
}
pub async fn serve_requests(engine: Engine) {
    let mut interval = tokio::time::interval(Duration::from_millis(200));
    while !engine.is_stopping() {
        interval.tick().await;
        let Ok(requests) = engine.store.terminal_requests() else {
            continue;
        };
        for value in requests {
            let Some(id) = value["id"].as_str() else {
                continue;
            };
            if !engine
                .store
                .update_terminal_request(id, "processing", Value::Null)
                .unwrap_or(false)
            {
                continue;
            }
            let result = prepare(
                &engine,
                value["request"]["id"].as_str().unwrap_or(""),
                value["request"]["confirmed"] == true,
            )
            .await;
            let value = match result {
                Ok(v) => v,
                Err(e) => json!({"error":e.to_string()}),
            };
            if !engine
                .store
                .update_terminal_request(id, "done", value.clone())
                .unwrap_or(false)
                && let Some(ticket) = value["ticket"].as_str()
            {
                cancel_ticket(&engine.store, ticket);
            }
        }
    }
}
pub async fn prepare_local(engine: &Engine, id: &str, confirmed: bool) -> Result<Value> {
    std::fs::create_dir_all(&engine.config.data_dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(engine.config.data_dir.join("service.lock"))?;
    match lock.try_lock() {
        Ok(()) => return prepare(engine, id, confirmed).await,
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(e) => return Err(e.into()),
    }
    let request = uuid::Uuid::new_v4().to_string();
    engine
        .store
        .queue_terminal_request(&request, &json!({"id":id,"confirmed":confirmed}))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        if let Some(value) = engine.store.terminal_request(&request)?
            && value["state"] == "done"
        {
            let result = value["result"].clone();
            if let Some(error) = result["error"].as_str() {
                bail!("{error}");
            }
            return Ok(result);
        }
        if tokio::time::Instant::now() > deadline {
            let _ = engine
                .store
                .update_terminal_request(&request, "cancelled", Value::Null);
            bail!(
                "실행 중인 BiBi가 터미널 전환에 응답하지 않았습니다. 앱을 최신 버전으로 갱신하고 다시 시도하세요."
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
pub async fn run_ticket(data_dir: &Path, ticket: &str) -> Result<i32> {
    uuid::Uuid::parse_str(ticket)?;
    #[cfg(target_os = "macos")]
    let _ = std::fs::remove_file(
        data_dir
            .join("terminal-launches")
            .join(format!("{ticket}.command")),
    );
    let store = Store::open(data_dir.join("bibi.sqlite3"))?;
    let plan: Ticket = serde_json::from_value(store.take_terminal_ticket(ticket)?)?;
    let result=async{
        let provider=store.provider(&plan.provider_id)?;
        if serde_json::to_value(&provider)?!=plan.provider_config {bail!("제공자 설정이 변경됐습니다. 다시 열어 주세요.");}
        let entry=Entry{id:String::new(),title:String::new(),provider:plan.provider.clone(),provider_id:Some(plan.provider_id.clone()),provider_name:String::new(),workspace:plan.workspace.clone(),project_key:None,project_name:String::new(),host_id:"local".into(),native_id:Some(plan.native.clone()),run_id:None,state:String::new(),updated_at:0,excerpt:String::new(),last_user:String::new(),last_assistant:String::new(),terminal_supported:true,terminal_reason:None,external:true,root:plan.root.clone(),file:plan.file.clone()};
        catalog::verify(&entry)?;
        if let Some(live)=owner(&entry)?&&live.state!=RunState::Disconnected{bail!("원래 세션이 다시 열렸습니다. 기존 창을 사용하세요.");}
        store.renew_terminal(&plan.provider,&plan.native,ticket,false)?;
        let mut cmd=launch::interactive_command(&provider.command)?;
        cmd.args(&plan.args).current_dir(&plan.workspace).env(if plan.provider==Provider::Claude{"CLAUDE_CONFIG_DIR"}else{"CODEX_HOME"},&plan.root).env_remove("BIBI_TOKEN").env_remove("BIBI_TOKEN_FILE").env_remove("BIBI_SERVER").stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit()).kill_on_drop(true);
        platform::stdio(&mut cmd)?;
        let mut child=cmd.spawn().context("원래 CLI를 시작하지 못했습니다.")?;
        let mut timer=tokio::time::interval(Duration::from_secs(5));
        loop {tokio::select!{status=child.wait()=>return Ok(status?.code().unwrap_or(1)),_=tokio::signal::ctrl_c()=>{/* The foreground native CLI receives Ctrl+C itself. Keep the lease until it actually exits. */},_=timer.tick()=>{if let Err(e)=store.renew_terminal(&plan.provider,&plan.native,ticket,false){let _=child.kill().await;return Err(e.into());}}}}
    }.await;
    let _ = store.renew_terminal(&plan.provider, &plan.native, ticket, true);
    result
}
pub async fn helper_entry() -> Option<i32> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).is_none_or(|a| a != "--bibi-terminal-run") {
        return None;
    }
    Some(if args.len() != 4 {
        2
    } else {
        match run_ticket(Path::new(&args[2]), &args[3]).await {
            Ok(code) => code,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }
    })
}

#[cfg(test)]
mod tests;
