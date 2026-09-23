use crate::runtime::{Control, Engine};
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::json;
use std::{process::Stdio, time::Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::mpsc,
};

pub async fn execute(
    engine: &Engine,
    run: Run,
    mut controls: mpsc::Receiver<Control>,
) -> Result<()> {
    let provider = engine
        .provider
        .as_ref()
        .context("실행 명령 설정이 없습니다.")?;
    if run.read_only {
        bail!("사용자 실행 명령의 읽기 전용 권한을 보장할 수 없습니다.");
    }
    let session = super::previous_session(engine, &run)?.unwrap_or_else(|| id("command"));
    let messages = super::openai::history(engine, &run)?;
    let prompt = serde_json::to_string(&messages)?;
    let args = provider.args.iter().map(|arg| {
        expand_argument(
            arg,
            &[
                ("prompt", &prompt),
                ("model", &run.model),
                ("workspace", &run.workspace),
                ("session_id", &session),
            ],
        )
    });
    let mut child = Command::new(&provider.command)
        .args(args)
        .current_dir(&run.workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("BIBI_SERVER")
        .env_remove("BIBI_TOKEN")
        .env_remove("BIBI_TOKEN_FILE")
        .kill_on_drop(true)
        .spawn()
        .context("등록한 실행 명령을 시작할 수 없습니다.")?;
    let start = Instant::now();
    let mut stdin = child.stdin.take().context("프로그램 입력 없음")?;
    let mut stdout = child.stdout.take().context("프로그램 출력 없음")?;
    let mut stderr = child.stderr.take().context("프로그램 오류 출력 없음")?;
    let input = serde_json::to_vec(
        &json!({"model":run.model,"prompt":run.context.question,"messages":messages,"session_id":session,"workspace":run.workspace}),
    )?;
    let writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.write_all(b"\n").await?;
        stdin.shutdown().await
    });
    let errors = tokio::spawn(async move {
        let mut result = Vec::new();
        let mut bytes = [0u8; 4096];
        loop {
            let n = stderr.read(&mut bytes).await?;
            if n == 0 {
                break;
            }
            let keep = n.min(8192 - result.len());
            result.extend_from_slice(&bytes[..keep]);
        }
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&result).into_owned())
    });
    engine.store.delivered(&run.id, &session, &id("turn"))?;
    let message = id("message");
    let mut bytes = [0u8; 8192];
    let mut pending = Vec::new();
    let mut output = String::new();
    loop {
        tokio::select! {
            count=stdout.read(&mut bytes)=>{
                let count=count?;if count==0 {break;}
                pending.extend_from_slice(&bytes[..count]);
                let valid=match std::str::from_utf8(&pending) {Ok(text)=>text.len(),Err(error) if error.error_len().is_none()=>error.valid_up_to(),Err(error)=>return Err(error.into())};
                let text=std::str::from_utf8(&pending[..valid])?;
                output.push_str(text);
                if output.len()>16*1024*1024 {bail!("프로그램 출력이 16 MiB를 초과했습니다.");}
                engine.store.append_output(&run.id,&message,"assistant",text)?;
                pending.drain(..valid);
            },
            control=controls.recv()=>match control {
                Some(Control::Respond{reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("이 실행 명령은 승인 입력을 제공하지 않습니다.")));},
                _=>{writer.abort();child.kill().await?;errors.abort();engine.store.fail(&run.id,"사용자가 중단했습니다.",true)?;return Ok(())}
            }
        }
    }
    if !pending.is_empty() {
        bail!("프로그램 출력의 UTF-8 문자가 완성되지 않았습니다.");
    }
    let status = loop {
        tokio::select! {
            result=child.wait()=>break result?,
            control=controls.recv()=>match control {
                Some(Control::Respond{reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("이 실행 명령은 승인 입력을 제공하지 않습니다.")));},
                _=>{writer.abort();errors.abort();child.kill().await?;engine.store.fail(&run.id,"사용자가 중단했습니다.",true)?;return Ok(())}
            }
        }
    };
    let input_result = tokio::time::timeout(std::time::Duration::from_secs(2), writer)
        .await
        .context("프로그램 입력 종료 시간 초과")??;
    let error = tokio::time::timeout(std::time::Duration::from_secs(2), errors)
        .await
        .context("프로그램 오류 출력 종료 시간 초과")???;
    if !status.success() {
        bail!("프로그램 종료 코드 {:?}: {}", status.code(), error);
    }
    // Programs using {prompt} arguments may close stdin without reading the JSON envelope.
    if let Err(error) = input_result
        && !provider.args.iter().any(|arg| arg.contains("{prompt}"))
    {
        return Err(error.into());
    }
    if output.trim().is_empty() {
        bail!("프로그램이 대화 출력을 반환하지 않았습니다. 비대화형 출력 명령이 필요합니다.");
    }
    engine.store.complete(
        &run.id,
        &output,
        UsageStats {
            duration_ms: Some(start.elapsed().as_millis() as u64),
            ..Default::default()
        },
    )?;
    Ok(())
}

pub fn expand_argument(input: &str, values: &[(&str, &str)]) -> String {
    let mut output = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(end) = rest.find('}') {
            if let Some((_, value)) = values.iter().find(|(key, _)| *key == &rest[1..end]) {
                output.push_str(value);
            } else {
                output.push_str(&rest[..=end]);
            }
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    output.push_str(rest);
    output
}
