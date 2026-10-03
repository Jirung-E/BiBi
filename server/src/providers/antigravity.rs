//! AGY's documented headless protocol. Never launch its interactive TUI.
use crate::runtime::{Control, Engine};
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::Value;
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::mpsc,
};

pub fn matches(command: &str) -> bool {
    command
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| matches!(name.to_ascii_lowercase().as_str(), "agy" | "agy.exe"))
}

struct Drain(tokio::task::JoinHandle<()>);
impl Drop for Drain {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn execute(
    engine: &Engine,
    run: Run,
    mut controls: mpsc::Receiver<Control>,
) -> Result<()> {
    let provider = engine
        .provider
        .as_ref()
        .context("AGY 실행 설정이 없습니다.")?;
    if run.read_only {
        bail!("AGY CLI의 읽기 전용 실행을 보장할 수 없습니다.");
    }
    for arg in &provider.args {
        let flag = arg.split('=').next().unwrap_or(arg);
        if matches!(
            flag,
            "-p" | "--print"
                | "--prompt"
                | "--input-format"
                | "--output-format"
                | "--continue"
                | "-c"
                | "--conversation"
                | "--dangerously-skip-permissions"
        ) {
            bail!(
                "AGY의 입력·출력·세션 옵션은 BiBi가 관리합니다. 실행 인자에서 {flag} 옵션을 제거하세요."
            );
        }
    }
    // Older IDE launchers also used the name agy. Probe help before giving one a prompt.
    let probe = async {
        let mut check = super::launch::command(&provider.command)?;
        check
            .args(&provider.args)
            .arg("--help")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = super::launch::spawn(&mut check)?;
        let mut output = String::new();
        child
            .stdout
            .take()
            .context("AGY 도움말 출력 없음")?
            .take(256 * 1024)
            .read_to_string(&mut output)
            .await?;
        let status = child.wait().await?;
        if !status.success()
            || !["--output-format", "--print", "--conversation"]
                .iter()
                .all(|flag| output.contains(flag))
        {
            bail!(
                "이 agy는 headless 대화를 지원하지 않습니다. Antigravity CLI의 실행 파일을 지정하세요. IDE 실행 명령으로는 채팅할 수 없습니다."
            );
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::pin!(probe);
    loop {
        tokio::select! {
            biased;
            control=controls.recv()=>match control {
                Some(Control::Respond{reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("AGY 연결 확인 중입니다.")));},
                _=>{engine.store.fail(&run.id,"질문 전송 전에 중단했습니다.",true)?;return Ok(())}
            },
            result=tokio::time::timeout(Duration::from_secs(10), &mut probe)=>{result.context("AGY 도움말 확인 시간 초과")??;break;}
        }
    }
    let previous = super::previous_session(engine, &run)?;
    if let Some(session) = &previous {
        uuid::Uuid::parse_str(session)
            .context("이전 AGY 연결은 네이티브 대화 ID가 없습니다. 새 세션으로 시작하세요.")?;
    }
    let mut command = super::launch::command(&provider.command)?;
    command
        .args(&provider.args)
        .args(["--output-format", "stream-json", "--print"])
        .arg(super::prompt(&run)?)
        .current_dir(&run.workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("BIBI_SERVER")
        .env_remove("BIBI_TOKEN")
        .env_remove("BIBI_TOKEN_FILE")
        .kill_on_drop(true);
    if let Some(session) = &previous {
        command.args(["--conversation", session]);
    }
    if !run.model.is_empty() {
        command.args(["--model", &run.model]);
    }
    let mut child = super::launch::spawn(&mut command)?;
    let mut lines = BufReader::new(child.stdout.take().context("AGY 출력 없음")?).lines();
    let mut stderr = child.stderr.take().context("AGY 오류 출력 없음")?;
    let _drain = Drain(tokio::spawn(async move {
        let mut bytes = [0; 4096];
        while let Ok(n) = stderr.read(&mut bytes).await {
            if n == 0 {
                break;
            }
        }
    }));
    let mut session = None::<String>;
    let mut output = String::new();
    let stream_id = id("agy_message");
    let start = tokio::time::Instant::now();
    let result = loop {
        tokio::select! {
            biased;
            control=controls.recv()=>match control {
                Some(Control::Respond {reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("AGY headless는 대화형 승인을 지원하지 않습니다.")));},
                _=>{child.kill().await?;engine.store.fail(&run.id,"사용자가 중단했습니다.",true)?;return Ok(())}
            },
            _=tokio::time::sleep_until(start+Duration::from_secs(30)),if session.is_none()=>{child.kill().await?;bail!("AGY 연결 시간 초과. headless를 지원하는 CLI 버전과 로그인을 확인하세요.");},
            line=lines.next_line()=>{
                let Some(line)=line? else {bail!("AGY가 완료 결과 없이 종료되었습니다. headless 지원 CLI 설치와 로그인을 확인하세요.")};
                if line.len()>16*1024*1024 {bail!("AGY 응답이 너무 큽니다.");}
                let event:Value=serde_json::from_str(&line).context("AGY stream-json 응답 형식이 아닙니다. 실행 파일과 CLI 버전을 확인하세요.")?;
                match event["event"].as_str() {
                    Some("init")=>{
                        let native=event["conversation_id"].as_str().context("AGY 대화 ID 없음")?;
                        uuid::Uuid::parse_str(native).context("잘못된 AGY 대화 ID")?;
                        if previous.as_deref().is_some_and(|old|old!=native)||session.as_deref().is_some_and(|old|old!=native) {bail!("AGY가 요청한 대화와 다른 세션을 열었습니다.");}
                        if session.is_none() {engine.store.delivered(&run.id,native,&run.request_id)?;}
                        session=Some(native.into());
                        if let Some(model)=event["init"]["model"].as_str() {engine.store.runtime_metadata(&run.id,Some(model),None,None)?;}
                    },
                    Some("step_update")=>{
                        let step=&event["step_update"];
                        if step["step_type"]=="agent_response" && session.is_some() && let Some(delta)=step["text_delta"].as_str() {
                            output.push_str(delta);if output.len()>16*1024*1024 {bail!("AGY 답변이 너무 큽니다.");}
                            engine.store.append_output(&run.id,&stream_id,"assistant",delta)?;
                        }
                    },
                    Some("result")=>break event["result"].clone(),
                    _=>(),
                }
            }
        }
    };
    // A completed headless result is authoritative. Reap a CLI that forgets to exit.
    if tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .is_err()
    {
        child.kill().await?;
    }
    if result["status"] != "SUCCESS" {
        engine.store.fail(
            &run.id,
            &format!(
                "AGY: {}",
                result["error"]
                    .as_str()
                    .unwrap_or("실행 실패 · CLI 로그인과 권한 정책을 확인하세요.")
            ),
            false,
        )?;
        return Ok(());
    }
    if session
        .as_deref()
        .is_none_or(|id| result["conversation_id"].as_str() != Some(id))
    {
        bail!("AGY 완료 결과의 대화 ID가 일치하지 않습니다.");
    }
    let text = result["response"].as_str().unwrap_or(&output);
    if output.is_empty() && !text.is_empty() {
        engine.store.add_message(&run.id, "assistant", text)?;
    }
    engine.store.complete(
        &run.id,
        text,
        UsageStats {
            input_tokens: result["usage"]["input_tokens"].as_u64(),
            cached_input_tokens: result["usage"]["cache_read_tokens"].as_u64(),
            output_tokens: result["usage"]["output_tokens"].as_u64(),
            duration_ms: Some(start.elapsed().as_millis() as u64),
        },
    )?;
    Ok(())
}
