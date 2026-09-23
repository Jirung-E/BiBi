use crate::runtime::{Control, Engine};
use anyhow::{Context, Result, bail};
use bibi_core::*;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub fn history(engine: &Engine, run: &Run) -> Result<Vec<Value>> {
    let mut messages = vec![json!({"role":"system","content":format!(
        "Role: {}. Preserve the user's constraints and conversation. This connection provides text conversation; do not claim to have used local tools unless the connected program actually provides them. Workspace: {}. Constraints: {}", run.role, run.workspace, serde_json::to_string(&run.context.constraints)?
    )})];
    for message in engine.store.detail(&run.id)?.conversation {
        if matches!(message.role.as_str(), "user" | "assistant") {
            messages.push(json!({"role":message.role,"content":message.text}));
        }
    }
    Ok(messages)
}

#[derive(Default)]
pub struct Sse {
    buffer: Vec<u8>,
    data: String,
}
impl Sse {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() + self.data.len() > 16 * 1024 * 1024 {
            bail!("API 응답 프레임이 너무 큽니다.");
        }
        let mut frames = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let bytes = self.buffer.drain(..=end).collect::<Vec<_>>();
            let line = std::str::from_utf8(&bytes)?.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    frames.push(std::mem::take(&mut self.data));
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
            }
        }
        Ok(frames)
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
        .context("API 제공자 설정이 없습니다.")?;
    if run.model.trim().is_empty() {
        bail!("사용할 모델을 선택하세요.");
    }
    let endpoint = if provider.endpoint.ends_with("/chat/completions") {
        provider.endpoint.clone()
    } else {
        format!(
            "{}/chat/completions",
            provider.endpoint.trim_end_matches('/')
        )
    };
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let request = engine.authorize(client.post(endpoint)).json(&json!({
        "model":run.model,"messages":history(engine,&run)?,"stream":true,"stream_options":{"include_usage":true}
    })).timeout(Duration::from_secs(1200));
    let start = Instant::now();
    let response = tokio::select! {
        response=request.send()=>response?,
        _=controls.recv()=>{engine.store.fail(&run.id,"요청 전 중단되었습니다.",true)?;return Ok(())}
    };
    if !response.status().is_success() {
        bail!(
            "API HTTP {}: {}",
            response.status(),
            response
                .text()
                .await?
                .chars()
                .take(1000)
                .collect::<String>()
        );
    }
    let session = super::previous_session(engine, &run)?.unwrap_or_else(|| id("api"));
    engine.store.delivered(&run.id, &session, &id("turn"))?;
    let mut state = Output {
        model: run.model.clone(),
        ..Default::default()
    };
    let message_id = id("message");
    if response
        .headers()
        .get("content-type")
        .and_then(|s| s.to_str().ok())
        .is_some_and(|s| s.contains("application/json"))
    {
        let value: Value = response.json().await?;
        consume(engine, &run, &message_id, &value, &mut state)?;
        state.done = true;
    } else {
        let mut stream = response.bytes_stream();
        let mut decoder = Sse::default();
        loop {
            tokio::select! {
                chunk=stream.next()=>{
                    let Some(chunk)=chunk else {break};
                    for frame in decoder.push(&chunk?)? {
                        if frame.trim()=="[DONE]" {state.done=true;break;}
                        consume(engine,&run,&message_id,&serde_json::from_str(&frame)?,&mut state)?;
                    }
                    if state.done {break;}
                },
                control=controls.recv()=>match control {
                    Some(Control::Respond{reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("이 API 연결은 승인 입력을 제공하지 않습니다.")));},
                    _=>{engine.store.fail(&run.id,"사용자가 중단했습니다.",true)?;return Ok(())}
                }
            }
        }
    }
    if !state.done && !state.finished {
        bail!("API 연결이 완료 확인 전에 종료되었습니다.");
    }
    if state.text.is_empty() {
        bail!("API가 표시할 텍스트를 반환하지 않았습니다.");
    }
    state.stats.duration_ms = Some(start.elapsed().as_millis() as u64);
    engine.store.complete(&run.id, &state.text, state.stats)?;
    Ok(())
}

#[derive(Default)]
struct Output {
    text: String,
    model: String,
    stats: UsageStats,
    done: bool,
    finished: bool,
}
fn consume(
    engine: &Engine,
    run: &Run,
    stream_id: &str,
    value: &Value,
    output: &mut Output,
) -> Result<()> {
    if let Some(error) = value.get("error") {
        bail!("API: {}", error["message"].as_str().unwrap_or("요청 실패"));
    }
    if let Some(model) = value["model"].as_str()
        && output.model != model
    {
        engine
            .store
            .runtime_metadata(&run.id, Some(model), None, None)?;
        output.model = model.into();
    }
    let usage = &value["usage"];
    if let Some(n) = usage["prompt_tokens"].as_u64() {
        output.stats.input_tokens = Some(n);
    }
    if let Some(n) = usage["completion_tokens"].as_u64() {
        output.stats.output_tokens = Some(n);
    }
    if let Some(n) = usage["prompt_tokens_details"]["cached_tokens"].as_u64() {
        output.stats.cached_input_tokens = Some(n);
    }
    let choice = &value["choices"][0];
    if choice["delta"]["tool_calls"]
        .as_array()
        .is_some_and(|v| !v.is_empty())
        || choice["message"]["tool_calls"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    {
        bail!("이 API 연결은 채팅 전용입니다. 도구 실행에는 CLI 연결을 사용하세요.");
    }
    if let Some(text) = choice["delta"]["content"]
        .as_str()
        .or_else(|| choice["message"]["content"].as_str())
        .or_else(|| choice["delta"]["refusal"].as_str())
    {
        output.text.push_str(text);
        if output.text.len() > 16 * 1024 * 1024 {
            bail!("응답이 16 MiB를 초과했습니다.");
        }
        engine
            .store
            .append_output(&run.id, stream_id, "assistant", text)?;
    }
    if !choice["finish_reason"].is_null() {
        output.finished = true;
    }
    Ok(())
}
