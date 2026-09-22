use super::{runtime_instructions, task_tools};
use crate::runtime::{Control, Engine};
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::mpsc,
};

// Native CLI transport; no Python/Node SDK runtime is required by the server.
pub(crate) struct Connection {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    pending: VecDeque<Value>,
    pub session: String,
}
impl Connection {
    fn alive(&mut self) -> bool {
        self.child.try_wait().is_ok_and(|v| v.is_none())
    }
    async fn send(&mut self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.stdin.write_all(&bytes).await?;
        self.stdin.flush().await?;
        Ok(())
    }
    async fn next(&mut self) -> Result<Value> {
        if let Some(value) = self.pending.pop_front() {
            return Ok(value);
        }
        let line = self.lines.next_line().await?.context(
            "Claude 연결이 종료되었습니다. 설치·로그인과 저장된 실행 상태를 확인하세요.",
        )?;
        if line.len() > 16 * 1024 * 1024 {
            bail!("Claude 응답 프레임이 너무 큽니다.")
        }
        Ok(serde_json::from_str(&line)?)
    }
    async fn reply(&mut self, request: &Value, response: Result<Value>) -> Result<()> {
        self.send(match response {
            Ok(value)=>json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":value}}),
            Err(error)=>json!({"type":"control_response","response":{"subtype":"error","request_id":request["request_id"],"error":error.to_string()}}),
        }).await
    }
    async fn start(engine: &Engine, run: &Run, previous: Option<String>) -> Result<Self> {
        let project = engine.store.project(&run.project_key)?;
        let session = previous
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        // Validate before turning a native session ID into a CLI option.
        uuid::Uuid::parse_str(&session).context("잘못된 Claude 세션 ID")?;
        let mut command = Command::new(&engine.config.claude_command);
        command
            .args([
                "--print",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--permission-mode",
                "manual",
                "--permission-prompt-tool",
                "stdio",
                "--strict-mcp-config",
            ])
            .arg("--mcp-config")
            .arg(json!({"mcpServers":{"bibi":{"type":"sdk","name":"bibi"}}}).to_string())
            .arg("--append-system-prompt")
            .arg(runtime_instructions(run, &project))
            .arg(if previous.is_some() {
                format!("--resume={session}")
            } else {
                format!("--session-id={session}")
            })
            .current_dir(&run.workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_remove("BIBI_SERVER")
            .env_remove("BIBI_TOKEN")
            .env_remove("BIBI_TOKEN_FILE")
            .kill_on_drop(true);
        if !run.model.trim().is_empty() {
            command.arg("--model").arg(&run.model);
        }
        if run.read_only {
            command.args(["--tools", ""]);
        }
        let mut child = command
            .spawn()
            .context("Claude Code를 시작할 수 없습니다. claude 설치·로그인을 확인하세요.")?;
        let stdin = child.stdin.take().context("Claude stdin 없음")?;
        let lines = BufReader::new(child.stdout.take().context("Claude stdout 없음")?).lines();
        let mut connection = Self {
            child,
            stdin,
            lines,
            pending: VecDeque::new(),
            session,
        };
        connection.send(json!({"type":"control_request","request_id":"bibi_initialize","request":{"subtype":"initialize","hooks":{},"forwardSubagentText":true}})).await?;
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut buffered = VecDeque::new();
            loop {
                let value = connection.next().await?;
                if value["type"] == "control_response"
                    && value["response"]["request_id"] == "bibi_initialize"
                {
                    if value["response"]["subtype"] != "success" {
                        bail!("Claude 초기화 실패: {}", value["response"]["error"])
                    }
                    connection.pending = buffered;
                    return Ok::<_, anyhow::Error>(());
                }
                if value["type"] == "control_request" {
                    let result = mcp_metadata(run, &value["request"]);
                    connection.reply(&value, result).await?;
                } else if value["type"] == "result" && value["is_error"] == true {
                    bail!("Claude: {}", result_text(&value));
                } else {
                    buffered.push_back(value);
                    if buffered.len() > 1000 {
                        bail!("Claude 초기화 대기열 초과")
                    }
                }
            }
        })
        .await
        .context("Claude 초기화 시간 초과")??;
        Ok(connection)
    }
}

fn mcp_metadata(run: &Run, request: &Value) -> Result<Value> {
    if request["subtype"] != "mcp_message" || request["server_name"] != "bibi" {
        bail!("지원하지 않는 Claude 제어 요청")
    }
    let msg = &request["message"];
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => {
            json!({"protocolVersion":msg["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),"capabilities":{"tools":{}},"serverInfo":{"name":"bibi","version":VERSION}})
        }
        "tools/list" => {
            json!({"tools":task_tools::definitions_for(run).as_array().unwrap().iter().map(|t|{
            let f=&t["function"];json!({"name":format!("bibi_{}",f["name"].as_str().unwrap()),"description":f["description"],"inputSchema":f["parameters"]})
        }).collect::<Vec<_>>()})
        }
        "notifications/initialized" | "ping" => json!({}),
        _ => bail!("지원하지 않는 MCP 메서드"),
    };
    Ok(json!({"mcp_response":{"jsonrpc":"2.0","id":msg["id"],"result":result}}))
}

pub async fn execute(
    engine: &Engine,
    run: Run,
    mut controls: mpsc::Receiver<Control>,
) -> Result<()> {
    let previous = super::previous_session(engine, &run)?;
    let cached = if previous.is_some() {
        engine.claude_sessions.take(run.session_id()).await
    } else {
        None
    };
    let mut connection = match cached {
        Some(mut connection) => {
            if connection.alive() {
                connection
            } else {
                Connection::start(engine, &run, previous).await?
            }
        }
        None => Connection::start(engine, &run, previous).await?,
    };
    engine.store.runtime_started(&run.id, &connection.session)?;
    connection.send(json!({"type":"user","uuid":uuid::Uuid::new_v4().to_string(),"session_id":connection.session,"parent_tool_use_id":null,"message":{"role":"user","content":super::prompt(&run)?},"client_composed":true})).await?;
    engine
        .store
        .delivered(&run.id, &connection.session, &run.request_id)?;
    let mut events = Events::default();
    let mut consults = 0;
    let mut interrupted = false;
    let mut interrupt_deadline = None;
    let mut result = None;
    loop {
        tokio::select! {
            value=connection.next()=>{
                let value=value?;
                if let Some(session)=value["session_id"].as_str() && session!=connection.session && value["parent_tool_use_id"].is_null() {bail!("Claude 응답의 세션 대상이 일치하지 않습니다.");}
                match value["type"].as_str().unwrap_or("") {
                    "control_request"=>{
                        let request=&value["request"];
                        match request["subtype"].as_str().unwrap_or("") {
                            "can_use_tool"=>{
                                let name=request["tool_name"].as_str().unwrap_or("");
                                if name.starts_with("mcp__bibi__bibi_") {
                                    connection.reply(&value,Ok(json!({"behavior":"allow","updatedInput":request["input"]}))).await?;
                                } else if run.read_only && name!="AskUserQuestion" {
                                    connection.reply(&value,Ok(json!({"behavior":"deny","message":"This BiBi session has read-only project access. Use the scoped BiBi tools."}))).await?;
                                } else {
                                    let mut detail=request.clone();detail["request_id"]=value["request_id"].clone();
                                    if name=="AskUserQuestion" {detail["questions"]=json!(request["input"]["questions"].as_array().unwrap_or(&vec![]).iter().enumerate().map(|(i,q)|json!({"id":i.to_string(),"question":q["question"],"options":q["options"]})).collect::<Vec<_>>());}
                                    engine.store.add_approval(Approval{id:id("approval"),run_id:run.id.clone(),native_id:value["request_id"].clone(),kind:if name=="AskUserQuestion"{"claude/requestUserInput"}else{"claude/requestApproval"}.into(),title:if name=="AskUserQuestion"{"Claude 추가 입력".into()}else{format!("Claude · {name}")},detail,state:"pending".into(),created_at:now()})?;
                                    engine.store.observe(&run.id,RunState::WaitingUser,"사용자 입력 대기",Some(name.into()))?;
                                }
                            },
                            "mcp_message" if request["server_name"]=="bibi"&&request["message"]["method"]=="tools/call"=>{
                                let msg=&request["message"];let name=msg["params"]["name"].as_str().unwrap_or("").strip_prefix("bibi_").unwrap_or("");
                                let key=format!("claude:tool:{}:{}",run.id,value["request_id"]);
                                let digest=serde_json::to_string(&msg["params"])?;
                                let record=if let Some(record)=engine.store.setting::<Value>(&key)? {
                                    if record["digest"]!=digest {bail!("같은 Claude 도구 ID에 다른 요청이 도착했습니다.");}record
                                }else{
                                    let outcome=tokio::select! {
                                        result=task_tools::execute(engine,&run,name,&msg["params"]["arguments"],&mut consults)=>result,
                                        control=controls.recv()=>{
                                            match control {Some(Control::Interrupt)=>{interrupted=true;interrupt_deadline=Some(tokio::time::Instant::now()+Duration::from_secs(10));connection.send(json!({"type":"control_request","request_id":"bibi_interrupt","request":{"subtype":"interrupt"}})).await?;},Some(Control::Respond{reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("업무 도구 실행 중입니다.")));},None=>()}
                                            Err(anyhow::anyhow!("업무 도구가 중단되었습니다."))
                                        }
                                    };
                                    let success=outcome.is_ok();let text=match outcome{Ok(v)=>v.to_string(),Err(e)=>json!({"error":e.to_string()}).to_string()};
                                    engine.store.add_message(&run.id,"tool",&format!("bibi_{name}\n{}",text.chars().take(4000).collect::<String>()))?;
                                    let record=json!({"digest":digest,"result":{"content":[{"type":"text","text":text}],"isError":!success}});
                                    engine.store.set_setting(&key,&record)?;record
                                };
                                connection.reply(&value,Ok(json!({"mcp_response":{"jsonrpc":"2.0","id":msg["id"],"result":record["result"]}}))).await?;
                            },
                            _=>connection.reply(&value,mcp_metadata(&run,request)).await?,
                        }
                    },
                    "control_cancel_request"=>{
                        for a in engine.store.detail(&run.id)?.approvals.into_iter().filter(|a|a.native_id==value["request_id"]&&a.state=="pending") {engine.store.approval_state(&a.id,"cancelled")?;}
                    },
                    "result"=>{result=Some(value);},
                    "rate_limit_event"=>{persist_quota(engine,&value["rate_limit_info"])?;},
                    _=>events.consume(engine,&run,&value)?,
                }
                if let Some(final_result)=result.as_ref() {
                    if events.active.is_empty() {
                        let stats=usage(&final_result["usage"],final_result["duration_ms"].as_u64());
                        if interrupted {engine.store.fail(&run.id,"사용자가 중단했습니다.",true)?;}
                        else if final_result["is_error"]==true {engine.store.fail(&run.id,&result_text(final_result),false)?;}
                        else {
                            let text=final_result["result"].as_str().filter(|s|!s.is_empty()).unwrap_or(&events.final_text);
                            if events.final_text.is_empty()&&!text.is_empty(){engine.store.add_message(&run.id,"assistant",text)?;}
                            engine.store.complete(&run.id,text,stats)?;
                        }
                        if !interrupted&&!engine.is_stopping(){engine.claude_sessions.put(run.session_id(),connection).await;}
                        return Ok(());
                    }
                    engine.store.observe(&run.id,RunState::WaitingExpert,"서브에이전트 대기",None)?;
                }
            },
            control=controls.recv()=>match control {
                Some(Control::Interrupt)=>{
                    interrupted=true;interrupt_deadline=Some(tokio::time::Instant::now()+Duration::from_secs(10));
                    connection.send(json!({"type":"control_request","request_id":"bibi_interrupt","request":{"subtype":"interrupt"}})).await?;
                },
                Some(Control::Respond{approval_id,value,reply})=>{let _=reply.send(respond(engine,&run,&mut connection,&approval_id,value).await);},
                None=>bail!("실행 제어 연결이 종료되었습니다."),
            },
            _=tokio::time::sleep(Duration::from_millis(250)),if interrupt_deadline.is_some()=>{
                if interrupt_deadline.is_some_and(|d|tokio::time::Instant::now()>=d) {bail!("Claude 중단 확인 시간이 초과되었습니다. 실행 확인이 필요합니다.");}
            }
        }
    }
}

async fn respond(
    engine: &Engine,
    run: &Run,
    connection: &mut Connection,
    id: &str,
    value: Value,
) -> Result<()> {
    let approval = engine.store.approval(id)?;
    if approval.run_id != run.id || approval.state != "pending" {
        bail!("현재 실행의 열린 요청이 아닙니다.")
    }
    let response = if approval.kind == "claude/requestUserInput" {
        let mut input = approval.detail["input"].clone();
        let mut answers = serde_json::Map::new();
        for (i, q) in input["questions"]
            .as_array()
            .context("질문이 없습니다.")?
            .iter()
            .enumerate()
        {
            let a = value["answers"][i.to_string()]["answers"]
                .as_array()
                .context("모든 질문에 답변이 필요합니다.")?;
            let texts = a
                .iter()
                .map(|v| v.as_str().context("텍스트 답변이 필요합니다."))
                .collect::<Result<Vec<_>>>()?;
            if texts.is_empty() || texts.iter().any(|s| s.trim().is_empty()) {
                bail!("빈 답변은 전달할 수 없습니다.");
            }
            answers.insert(
                q["question"].as_str().context("질문 문구 없음")?.into(),
                json!(texts.join(", ")),
            );
        }
        input["answers"] = Value::Object(answers);
        json!({"behavior":"allow","updatedInput":input})
    } else {
        match value["decision"].as_str() {
            Some("accept") => json!({"behavior":"allow","updatedInput":approval.detail["input"]}),
            Some("decline") => {
                json!({"behavior":"deny","message":"The user declined this request."})
            }
            _ => bail!("승인 또는 거절을 선택하세요."),
        }
    };
    engine.store.approval_state(id, "sending")?;
    if let Err(e) = connection
        .reply(&json!({"request_id":approval.native_id}), Ok(response))
        .await
    {
        engine.store.approval_state(id, "uncertain")?;
        return Err(e);
    }
    engine.store.approval_state(id, "delivered")?;
    if !engine
        .store
        .detail(&run.id)?
        .approvals
        .iter()
        .any(|a| a.state == "pending")
    {
        engine
            .store
            .observe(&run.id, RunState::Running, "응답 전달됨", None)?;
    }
    Ok(())
}

fn result_text(value: &Value) -> String {
    value["result"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| {
            value["errors"]
                .as_array()
                .map(|e| {
                    e.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_else(|| "Claude 실행 실패".into())
        })
}
fn usage(value: &Value, duration: Option<u64>) -> UsageStats {
    UsageStats {
        input_tokens: value["input_tokens"].as_u64().map(|v| {
            v + value["cache_creation_input_tokens"].as_u64().unwrap_or(0)
                + value["cache_read_input_tokens"].as_u64().unwrap_or(0)
        }),
        cached_input_tokens: value["cache_read_input_tokens"].as_u64(),
        output_tokens: value["output_tokens"].as_u64(),
        duration_ms: duration,
    }
}

#[derive(Default)]
pub struct Events {
    active: HashSet<String>,
    agents: HashMap<String, String>,
    owners: HashMap<String, String>,
    tasks: HashMap<String, String>,
    background: HashSet<String>,
    streams: HashMap<String, String>,
    final_text: String,
}
impl Events {
    pub fn consume(&mut self, engine: &Engine, run: &Run, value: &Value) -> Result<()> {
        let parent = value["parent_tool_use_id"].as_str().unwrap_or("");
        let event_id = value["uuid"].as_str().unwrap_or("event");
        let target = if parent.is_empty() {
            run.clone()
        } else if let Some(id) = self.agents.get(parent) {
            engine.store.run(id)?
        } else {
            engine.store.observe_subagent(
                &run.id,
                SubagentUpdate {
                    native_id: parent.into(),
                    event_id: event_id.into(),
                    title: String::new(),
                    state: None,
                    prompt: None,
                    text: None,
                    stats: None,
                    started: false,
                },
            )?
        };
        match value["type"].as_str().unwrap_or("") {
            "stream_event" => {
                let event = &value["event"];
                if event["type"] == "message_start" {
                    self.streams.insert(
                        parent.into(),
                        event["message"]["id"].as_str().unwrap_or(event_id).into(),
                    );
                }
                if event["delta"]["type"] == "text_delta" {
                    let key = format!(
                        "{}:{}",
                        target.id,
                        self.streams
                            .get(parent)
                            .map(String::as_str)
                            .unwrap_or(event_id)
                    );
                    engine.store.append_output(
                        &target.id,
                        &key,
                        "assistant",
                        event["delta"]["text"].as_str().unwrap_or(""),
                    )?;
                }
            }
            "assistant" => {
                let blocks = value["message"]["content"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let text = blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    let key = format!(
                        "{}:{}",
                        target.id,
                        value["message"]["id"].as_str().unwrap_or(event_id)
                    );
                    engine.store.set_message(Message {
                        id: key,
                        run_id: target.id.clone(),
                        role: "assistant".into(),
                        text: text.clone(),
                        created_at: now(),
                    })?;
                    if parent.is_empty() {
                        self.final_text = text;
                    }
                }
                for block in blocks.iter().filter(|b| b["type"] == "tool_use") {
                    let tool = block["name"].as_str().unwrap_or("tool");
                    let native = block["id"].as_str().unwrap_or(event_id);
                    if ["Agent", "Task"].contains(&tool) {
                        self.active.insert(native.into());
                        if block["input"]["run_in_background"] == true {
                            self.background.insert(native.into());
                        }
                        let child = engine.store.observe_subagent(
                            &target.id,
                            SubagentUpdate {
                                native_id: native.into(),
                                event_id: native.into(),
                                title: block["input"]["description"]
                                    .as_str()
                                    .unwrap_or("Claude 서브에이전트")
                                    .into(),
                                state: Some(RunState::Running),
                                prompt: block["input"]["prompt"].as_str().map(String::from),
                                text: None,
                                stats: None,
                                started: true,
                            },
                        )?;
                        self.agents.insert(native.into(), child.id);
                        self.owners.insert(native.into(), target.id.clone());
                    } else {
                        engine.store.set_message(Message {
                            id: format!("{}:tool:{native}", target.id),
                            run_id: target.id.clone(),
                            role: "tool".into(),
                            text: format!("{tool}\n{}", block["input"]),
                            created_at: now(),
                        })?;
                    }
                }
            }
            "user" => {
                for block in value["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "tool_result")
                {
                    let native = block["tool_use_id"].as_str().unwrap_or("");
                    if self.active.contains(native) && !self.background.contains(native) {
                        let text =
                            block["content"]
                                .as_str()
                                .map(String::from)
                                .unwrap_or_else(|| {
                                    block["content"]
                                        .as_array()
                                        .map(|v| {
                                            v.iter()
                                                .filter_map(|b| b["text"].as_str())
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        })
                                        .unwrap_or_default()
                                });
                        engine.store.observe_subagent(
                            self.owners
                                .get(native)
                                .map(String::as_str)
                                .unwrap_or(&target.id),
                            SubagentUpdate {
                                native_id: native.into(),
                                event_id: event_id.into(),
                                title: String::new(),
                                state: Some(if block["is_error"] == true {
                                    RunState::Failed
                                } else {
                                    RunState::Completed
                                }),
                                prompt: None,
                                text: Some(text),
                                stats: None,
                                started: false,
                            },
                        )?;
                        self.active.remove(native);
                    }
                }
            }
            "system" => {
                let subtype = value["subtype"].as_str().unwrap_or("");
                if ![
                    "task_started",
                    "task_progress",
                    "task_notification",
                    "task_updated",
                ]
                .contains(&subtype)
                {
                    return Ok(());
                }
                let task = value["task_id"].as_str().unwrap_or("");
                let native = value["tool_use_id"]
                    .as_str()
                    .or_else(|| self.tasks.get(task).map(String::as_str))
                    .unwrap_or(task)
                    .to_owned();
                if subtype == "task_started" {
                    let kind = value["task_type"].as_str().unwrap_or("");
                    if !self.active.contains(&native) && !kind.contains("agent") {
                        return Ok(());
                    }
                    self.tasks.insert(task.into(), native.clone());
                    self.active.insert(native.clone());
                    self.background.insert(native.clone());
                } else if !self.active.contains(&native) && !self.tasks.contains_key(task) {
                    return Ok(());
                }
                let status = value["status"]
                    .as_str()
                    .or_else(|| value["patch"]["status"].as_str())
                    .unwrap_or("running");
                let state = match status {
                    "completed" => RunState::Completed,
                    "failed" => RunState::Failed,
                    "stopped" | "killed" => RunState::Interrupted,
                    _ => RunState::Running,
                };
                if state.terminal() {
                    self.active.remove(&native);
                }
                let child = engine.store.observe_subagent(
                    self.owners
                        .get(&native)
                        .map(String::as_str)
                        .unwrap_or(&run.id),
                    SubagentUpdate {
                        native_id: native.clone(),
                        event_id: event_id.into(),
                        title: value["description"].as_str().unwrap_or("").into(),
                        state: Some(state),
                        prompt: None,
                        text: value["summary"]
                            .as_str()
                            .or_else(|| value["patch"]["result"].as_str())
                            .map(String::from),
                        stats: None,
                        started: subtype == "task_started",
                    },
                )?;
                self.agents.insert(native.clone(), child.id);
                self.owners.entry(native).or_insert_with(|| run.id.clone());
            }
            _ => (),
        }
        Ok(())
    }
}

pub fn persist_quota(engine: &Engine, info: &Value) -> Result<()> {
    let kind = info["rateLimitType"].as_str().unwrap_or("unknown");
    let (label, duration) = match kind {
        "five_hour" => ("5시간", Some(300)),
        "seven_day" => ("주간", Some(10080)),
        "seven_day_opus" => ("Opus 주간", Some(10080)),
        "seven_day_sonnet" => ("Sonnet 주간", Some(10080)),
        "overage" => ("추가 사용량", None),
        _ => ("한도", None),
    };
    let mut quota = engine
        .store
        .quotas()?
        .into_iter()
        .find(|q| q.id == "local:Claude")
        .unwrap_or(Quota {
            id: "local:Claude".into(),
            provider: Provider::Claude,
            account: "Claude Code 로그인 계정".into(),
            host_id: "local".into(),
            model: None,
            status: "unknown".into(),
            windows: vec![],
            observed_at: None,
            reason: None,
        });
    quota.windows.retain(|w| w.label != label);
    quota.windows.push(QuotaWindow {
        label: label.into(),
        remaining_percent: info["utilization"]
            .as_f64()
            .filter(|v| v.is_finite())
            .map(|v| ((1.0 - v) * 100.0).clamp(0.0, 100.0)),
        duration_minutes: duration,
        resets_at: info["resetsAt"].as_i64().map(|v| v * 1000),
    });
    quota.status = if info["status"] == "rejected" {
        "limited"
    } else {
        "connected"
    }
    .into();
    quota.observed_at = Some(now());
    quota.reason = if info["status"] == "rejected" {
        Some("Claude 사용 한도에 도달했습니다.".into())
    } else {
        None
    };
    engine.store.upsert_quota(quota)?;
    Ok(())
}
pub async fn refresh(engine: &Engine) -> Result<Value> {
    let result = async {
        let output = tokio::time::timeout(
            Duration::from_secs(15),
            Command::new(&engine.config.claude_command)
                .args(["auth", "status", "--json"])
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("Claude 로그인 확인 시간 초과")??;
        let data: Value = serde_json::from_slice(&output.stdout)
            .context("Claude 로그인 상태를 읽을 수 없습니다.")?;
        if !output.status.success() || data["loggedIn"] != true {
            bail!("Claude Code 로그인이 필요합니다. 이 호스트에서 claude auth login을 실행하세요.");
        }
        Ok::<_, anyhow::Error>(data)
    }
    .await;
    let mut quota = engine
        .store
        .quotas()?
        .into_iter()
        .find(|q| q.id == "local:Claude")
        .unwrap_or(Quota {
            id: "local:Claude".into(),
            provider: Provider::Claude,
            account: "Claude Code".into(),
            host_id: "local".into(),
            model: None,
            status: "unknown".into(),
            windows: vec![],
            observed_at: None,
            reason: None,
        });
    match result {
        Ok(data) => {
            quota.account = format!(
                "Claude Code · {}",
                data["subscriptionType"].as_str().unwrap_or("로그인됨")
            );
            if quota.windows.is_empty() {
                quota.status = "connected".into();
                quota.reason = Some("로그인됨 · 사용 한도는 Claude가 제공할 때 표시됩니다.".into());
            }
            engine.store.upsert_quota(quota)?;
            Ok(json!({"status":"connected","logged_in":true}))
        }
        Err(e) => {
            quota.status = "error".into();
            quota.reason = Some(e.to_string());
            engine.store.upsert_quota(quota)?;
            Err(e)
        }
    }
}
