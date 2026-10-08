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
    process::{Child, ChildStdin, ChildStdout},
    sync::mpsc,
};

fn permission_mode(run: &Run) -> &'static str {
    match run.approval_mode {
        // The SDK wire value is default; the manual CLI alias requires >= 2.1.200.
        ApprovalMode::OnRequest => "default",
        ApprovalMode::AcceptEdits => "acceptEdits",
        ApprovalMode::FullAccess => "bypassPermissions",
    }
}

// Native CLI transport; no Python/Node SDK runtime is required by the server.
pub(crate) struct Connection {
    configuration_revision: u64,
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    pending: VecDeque<Value>,
    diagnostics: super::claude_diagnostics::Diagnostics,
    pub session: String,
}
impl Connection {
    fn alive(&mut self) -> bool {
        self.child.try_wait().is_ok_and(|v| v.is_none())
    }
    async fn send(&mut self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        if self.stdin.write_all(&bytes).await.is_err() || self.stdin.flush().await.is_err() {
            return Err(self.transport_error().await);
        }
        Ok(())
    }
    async fn next(&mut self) -> Result<Value> {
        if let Some(value) = self.pending.pop_front() {
            return Ok(value);
        }
        let line = match self.lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => return Err(self.transport_error().await),
        };
        if line.len() > 32 * 1024 * 1024 {
            bail!("Claude 응답 프레임이 너무 큽니다.")
        }
        Ok(serde_json::from_str(&line)?)
    }
    async fn transport_error(&mut self) -> anyhow::Error {
        // EOF can precede the process-exit notification. Wait briefly, never
        // infer process death merely from a closed output/control pipe.
        let status = tokio::time::timeout(Duration::from_millis(200), self.child.wait()).await;
        let reason = match status {
            Ok(Ok(status)) => {
                self.diagnostics.finish().await;
                match status.code() {
                    Some(code) => format!("Claude 프로세스가 종료되었습니다 (종료 코드: {code})."),
                    None => "Claude 프로세스가 종료되었습니다 (시그널 종료).".into(),
                }
            }
            _ => "Claude 제어 연결이 끊겼습니다. 프로세스 종료는 확인되지 않았습니다.".into(),
        };
        anyhow::anyhow!("{reason} {}", self.diagnostics.hint())
    }
    async fn reply(&mut self, request: &Value, response: Result<Value>) -> Result<()> {
        self.send(match response {
            Ok(value)=>json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":value}}),
            Err(error)=>json!({"type":"control_response","response":{"subtype":"error","request_id":request["request_id"],"error":error.to_string()}}),
        }).await
    }
    async fn set_model(&mut self, run: &Run) -> Result<()> {
        let model = if run.model.trim().is_empty() {
            Value::Null
        } else {
            json!(run.model)
        };
        self.set_option(
            run,
            "model",
            json!({"subtype":"set_model","model":model}),
            "모델",
        )
        .await
    }
    async fn set_approval_mode(&mut self, run: &Run) -> Result<()> {
        self.set_option(
            run,
            "permission",
            json!({"subtype":"set_permission_mode","mode":permission_mode(run)}),
            "승인 모드",
        )
        .await
    }
    async fn set_option(
        &mut self,
        run: &Run,
        key: &str,
        request: Value,
        label: &str,
    ) -> Result<()> {
        let id = format!("bibi_{key}_{}", run.request_id);
        self.send(json!({"type":"control_request","request_id":id,"request":request}))
            .await?;
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut buffered = VecDeque::new();
            loop {
                let value = self.next().await?;
                if value["type"] == "control_response" && value["response"]["request_id"] == id {
                    if value["response"]["subtype"] != "success" {
                        bail!("Claude {label} 변경 실패: {}", value["response"]["error"]);
                    }
                    // Metadata received before the acknowledgement still belongs
                    // to the previous turn. Leave its ordering intact.
                    buffered.append(&mut self.pending);
                    self.pending = buffered;
                    return Ok::<_, anyhow::Error>(());
                }
                if value["type"] == "control_request" {
                    self.reply(&value, mcp_metadata(run, &value["request"]))
                        .await?;
                } else {
                    buffered.push_back(value);
                    if buffered.len() > 1000 {
                        bail!("Claude {label} 변경 대기열 초과");
                    }
                }
            }
        })
        .await
        .with_context(|| format!("Claude {label} 변경 시간 초과"))?
    }
    async fn start(engine: &Engine, run: &Run, previous: Option<String>) -> Result<Self> {
        let project = engine.store.project(&run.project_key)?;
        let configuration_revision = engine.project_revision(&run.project_key).await;
        let session = previous
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        // Validate before turning a native session ID into a CLI option.
        uuid::Uuid::parse_str(&session).context("잘못된 Claude 세션 ID")?;
        let mut command = super::launch::command(&engine.config.claude_command)?;
        command
            .args(&engine.config.claude_args)
            .args([
                "--print",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--permission-mode",
                permission_mode(run),
                "--permission-prompt-tool",
                "stdio",
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
            .stderr(Stdio::piped())
            .env_remove("BIBI_SERVER")
            .env_remove("BIBI_TOKEN")
            .env_remove("BIBI_TOKEN_FILE")
            .kill_on_drop(true);
        if previous.is_none() {
            command.arg("--name").arg(format!("BiBi · {}", run.title));
        }
        if !run.model.trim().is_empty() {
            command.arg("--model").arg(&run.model);
        }
        run.approval_mode.validate(&run.provider, run.read_only)?;
        if run.read_only {
            command.args(["--tools", "", "--strict-mcp-config"]);
        } else {
            // Enable later user-selected transitions; this does not activate bypass.
            command.arg("--allow-dangerously-skip-permissions");
        }
        if let Some(source) = engine
            .store
            .setting::<Value>(&format!("external_resume:{}", run.session_id()))?
            && let Some(root) = source["claude_root"].as_str()
        {
            command.env("CLAUDE_CONFIG_DIR", root);
        }
        if let Some(root) = engine
            .store
            .setting::<String>(&format!("claude_root:{}", run.session_id()))?
        {
            command.env("CLAUDE_CONFIG_DIR", root);
        }
        let mut child = super::launch::spawn(&mut command)?;
        let stdin = child.stdin.take().context("Claude stdin 없음")?;
        let lines = BufReader::new(child.stdout.take().context("Claude stdout 없음")?).lines();
        let diagnostics = super::claude_diagnostics::Diagnostics::start(
            child.stderr.take().context("Claude stderr 없음")?,
        );
        let mut connection = Self {
            configuration_revision,
            child,
            diagnostics,
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
                    let metadata = &value["response"]["response"];
                    if metadata["commands"].is_array() {
                        engine.store.runtime_metadata(
                            &run.id,
                            None,
                            Some(commands(&metadata["commands"])),
                            None,
                        )?;
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
    if let Some(session) = &previous {
        engine.store.runtime_started(&run.id, session)?;
    }
    let revision = engine.project_revision(&run.project_key).await;
    let cached = if previous.is_some() {
        engine.claude_sessions.take(run.session_id()).await
    } else {
        None
    };
    let prepare = async {
        let connection = match cached {
            Some(mut connection) => {
                if connection.configuration_revision == revision && connection.alive() {
                    let prior = engine
                        .store
                        .run(run.continued_from.as_deref().context("이전 실행 없음")?)?;
                    if prior.approval_mode != run.approval_mode
                        && let Err(error) = connection.set_approval_mode(&run).await
                    {
                        // No user input was sent. Keep the native identity so the user can
                        // correct the mode and continue this conversation after the process closes.
                        engine.store.runtime_started(&run.id, &connection.session)?;
                        engine.store.fail(
                            &run.id,
                            &format!("{error}. 질문은 전송하지 않았습니다."),
                            false,
                        )?;
                        return Err(error);
                    }
                    if prior.model != run.model {
                        connection.set_model(&run).await?;
                    }
                    connection
                } else {
                    Connection::start(engine, &run, previous).await?
                }
            }
            None => Connection::start(engine, &run, previous).await?,
        };
        Ok::<_, anyhow::Error>(connection)
    };
    tokio::pin!(prepare);
    let mut connection = loop {
        tokio::select! {
            biased;
            control = controls.recv() => match control {
                Some(Control::Respond {reply,..}) => {let _ = reply.send(Err(anyhow::anyhow!("Claude 연결을 준비하고 있습니다.")));},
                _ => {engine.store.fail(&run.id,"질문 전송 전에 중단했습니다.",true)?; return Ok(());}
            },
            result = &mut prepare => match result {
                Ok(connection)=>break connection,
                Err(error)=>{engine.store.fail(&run.id,&format!("{error}. 질문은 전송하지 않았습니다."),false)?;return Err(error);}
            },
        }
    };
    let prompt = super::prompt(&run)?;
    if let Some((name, _)) = bibi_core::slash::parse(&run.context.question) {
        let known = engine.store.run(&run.id)?.runtime.commands;
        if !known.iter().any(|c| c.name == name) {
            engine.store.runtime_started(&run.id, &connection.session)?;
            engine.store.fail(&run.id,&format!("이 Claude 세션에서 /{name} 명령을 지원하지 않습니다. 질문은 전송하지 않았습니다."),false)?;
            engine
                .claude_sessions
                .put(run.session_id(), connection)
                .await;
            return Ok(());
        }
    }
    engine.store.runtime_started(&run.id, &connection.session)?;
    let frame = json!({"type":"user","uuid":uuid::Uuid::new_v4().to_string(),"session_id":connection.session,"parent_tool_use_id":null,"message":{"role":"user","content":crate::attachments::claude_content(engine,&prompt,&run.context.attachments)?},"client_composed":true});
    let sent = {
        let send = connection.send(frame);
        tokio::pin!(send);
        loop {
            tokio::select! {
                biased;
                control=controls.recv()=>match control {
                    Some(Control::Respond{reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("질문 전달 중입니다.")));},
                    _=>break false,
                },
                result=&mut send=>{result?;break true;}
            }
        }
    };
    if !sent {
        connection.child.kill().await?;
        engine.store.fail(&run.id, "사용자가 중단했습니다.", true)?;
        return Ok(());
    }
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
                if value["type"]=="system" && value["subtype"]=="init" && value["parent_tool_use_id"].is_null() {
                    let commands=value.get("slash_commands").filter(|v|v.is_array()).map(commands);
                    engine.store.runtime_metadata(&run.id,value["model"].as_str(),commands,value["transcript_path"].as_str().map(String::from))?;
                }
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
                        let session_file=super::claude_usage::session_file(&connection.session).await;
                        if session_file.is_some() {engine.store.runtime_metadata(&run.id,None,None,session_file)?;}
                        if engine.store.run(&run.id)?.model.is_empty() && let Some(model)=final_result["modelUsage"].as_object().filter(|v|v.len()==1).and_then(|v|v.keys().next()) {engine.store.runtime_metadata(&run.id,Some(model),None,None)?;}
                        if !interrupted&&!engine.is_stopping(){engine.claude_sessions.put(run.session_id(),connection).await;}
                        if engine.provider.is_some() { let worker=engine.clone();tokio::spawn(async move { let _=super::claude_usage::refresh(&worker).await; }); }
                        return Ok(());
                    }
                    if !engine.store.detail(&run.id)?.approvals.iter().any(|a|a.state=="pending") {
                        engine.store.observe(&run.id,RunState::WaitingExpert,"서브에이전트 대기",None)?;
                    }
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
                if interrupt_deadline.is_some_and(|d|tokio::time::Instant::now()>=d) {
                    connection.child.kill().await.context("Claude 프로세스 중단 실패")?;
                    engine.store.fail(&run.id,"중단 응답이 없어 Claude 프로세스를 종료했습니다.",true)?;
                    return Ok(());
                }
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

fn historical_agent_ids(run: &Run) -> HashMap<String, String> {
    use std::io::BufRead;
    let mut ids = HashMap::new();
    let Some(path) = run.runtime.session_file.as_ref().map(std::path::Path::new) else {
        return ids;
    };
    let Some(session) = run
        .session_key
        .as_ref()
        .filter(|s| uuid::Uuid::parse_str(s).is_ok())
    else {
        return ids;
    };
    if path.file_name().and_then(|s| s.to_str()) != Some(format!("{session}.jsonl").as_str()) {
        return ids;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return ids;
    };
    if file
        .metadata()
        .map_or(true, |m| !m.is_file() || m.len() > 32 * 1024 * 1024)
    {
        return ids;
    }
    for line in std::io::BufReader::new(file).lines().take(50000) {
        let Ok(line) = line else {
            break;
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        for block in value["message"]["content"].as_array().into_iter().flatten() {
            if block["type"] == "tool_result"
                && let Some(tool) = block["tool_use_id"].as_str()
                && let Some(native) = agent_result_id(&value, block)
            {
                ids.insert(tool.into(), native);
            } else if block["type"] == "tool_use"
                && block["name"] == "SendMessage"
                && let Some(tool) = block["id"].as_str()
                && let Some(native) = block["input"]["to"]
                    .as_str()
                    .or_else(|| block["input"]["recipient"].as_str())
            {
                ids.insert(tool.into(), native.into());
            }
        }
    }
    ids
}

// Reconcile only identities present in recorded public tool results/messages.
// Never infer identity from a display name or read private model state.
pub fn reconcile_history(store: &Store) -> Result<()> {
    if store.setting::<bool>("claude_agent_identity_v1")? == Some(true) {
        return Ok(());
    }
    let all = store.snapshot()?.runs;
    let mut parents = HashMap::<String, Run>::new();
    for run in &all {
        if run.provider == Provider::Claude
            && run.agent_kind != "subagent"
            && run.host_id == "local"
            && parents
                .get(run.session_id())
                .is_none_or(|old| old.created_at < run.created_at)
        {
            parents.insert(run.session_id().into(), run.clone());
        }
    }
    for parent in parents.values() {
        let mut recipients = historical_agent_ids(parent);
        for message in store.detail(&parent.id)?.conversation {
            if let Some(body) = message.text.strip_prefix("SendMessage\n")
                && let Ok(args) = serde_json::from_str::<Value>(body)
                && let Some(id) = args["to"].as_str().or_else(|| args["recipient"].as_str())
                && let Some((_, tool)) = message.id.rsplit_once(":tool:")
            {
                recipients.insert(tool.into(), id.into());
            }
        }
        let mut identities = HashMap::<String, Vec<String>>::new();
        for child in all.iter().filter(|r| {
            r.agent_kind == "subagent"
                && r.parent_session_id.as_deref() == Some(parent.session_id())
                && r.state.terminal()
        }) {
            let recorded = store
                .detail(&child.id)?
                .messages
                .into_iter()
                .filter(|m| m.role == "assistant")
                .find_map(|m| agent_result_id(&Value::Null, &json!({"content":m.text})));
            if let Some(native) = recorded.or_else(|| {
                child
                    .session_key
                    .as_ref()
                    .and_then(|key| recipients.get(key).cloned())
            }) {
                identities.entry(native).or_default().push(child.id.clone());
            }
        }
        for (native, ids) in identities {
            // A nested historical tree needs its own alias migration; leave it
            // intact rather than guessing links from incomplete old telemetry.
            if all.iter().any(|r| {
                r.parent_session_id
                    .as_ref()
                    .is_some_and(|id| ids.contains(id))
            }) {
                continue;
            }
            store.reconcile_subagent_history(&parent.id, &native, &ids)?;
        }
    }
    store.set_setting("claude_agent_identity_v1", &true)?;
    Ok(())
}

fn agent_result_id(value: &Value, block: &Value) -> Option<String> {
    for key in ["tool_use_result", "toolUseResult"] {
        if let Some(id) = value[key]["agentId"]
            .as_str()
            .or_else(|| value[key]["resumedAgentId"].as_str())
        {
            return Some(id.into());
        }
    }
    // Older CLI streams put the public resume ID in the tool-result text.
    let text = match &block["content"] {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let id = text.split("agentId:").nth(1)?.split_whitespace().next()?;
    (!id.is_empty()
        && id.len() <= 256
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then(|| id.into())
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
        if let Some(model) = value["message"]["model"]
            .as_str()
            .or_else(|| value["event"]["message"]["model"].as_str())
            && engine.store.run(&target.id)?.model != model
        {
            engine
                .store
                .runtime_metadata(&target.id, Some(model), None, None)?;
        }
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
                        attachments: vec![],
                        phase: None,
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
                    let recipient = if tool == "SendMessage"
                        && block["input"]["type"]
                            .as_str()
                            .is_none_or(|kind| kind == "message")
                    {
                        block["input"]["to"]
                            .as_str()
                            .or_else(|| block["input"]["recipient"].as_str())
                    } else {
                        None
                    };
                    if ["Agent", "Task"].contains(&tool) || recipient.is_some() {
                        let resumed = recipient.or_else(|| block["input"]["resume"].as_str());
                        if let Some(id) = resumed
                            && let Some(agent) = engine.store.subagent_by_native(&target.id, id)?
                        {
                            engine.store.alias_subagent(&target.id, native, &agent.id)?;
                        }
                        self.active.insert(native.into());
                        if recipient.is_some() || block["input"]["run_in_background"] == true {
                            self.background.insert(native.into());
                        }
                        let child = engine.store.observe_subagent(
                            &target.id,
                            SubagentUpdate {
                                native_id: if engine
                                    .store
                                    .subagent_by_native(&target.id, native)?
                                    .is_some()
                                {
                                    native
                                } else {
                                    resumed.unwrap_or(native)
                                }
                                .into(),
                                event_id: native.into(),
                                title: block["input"]["description"].as_str().unwrap_or("").into(),
                                state: Some(RunState::Running),
                                prompt: block["input"]["prompt"]
                                    .as_str()
                                    .or_else(|| block["input"]["message"].as_str())
                                    .or_else(|| block["input"]["content"].as_str())
                                    .map(String::from),
                                text: None,
                                stats: None,
                                started: true,
                            },
                        )?;
                        engine.store.alias_subagent(&target.id, native, &child.id)?;
                        if let Some(id) = resumed {
                            engine.store.alias_subagent(&target.id, id, &child.id)?;
                        }
                        self.agents.insert(native.into(), child.id);
                        self.owners.insert(native.into(), target.id.clone());
                    } else {
                        engine.store.set_message(Message {
                            attachments: vec![],
                            phase: None,
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
                    if let Some(child) = self.agents.get(native) {
                        if let Some(agent) = agent_result_id(value, block) {
                            engine.store.alias_subagent(
                                self.owners
                                    .get(native)
                                    .map(String::as_str)
                                    .unwrap_or(&target.id),
                                &agent,
                                child,
                            )?;
                        }
                        if block["is_error"] == true {
                            self.background.remove(native);
                        }
                    }
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
        .find(|q| q.id == engine.quota_id("Claude"))
        .unwrap_or(Quota {
            id: engine.quota_id("Claude"),
            provider: Provider::Claude,
            provider_id: engine.provider_id(),
            account: "Claude Code 로그인 계정".into(),
            host_id: "local".into(),
            model: None,
            status: "unknown".into(),
            windows: vec![],
            observed_at: None,
            reason: None,
        });
    let previous = quota.windows.iter().find(|w| w.label == label).cloned();
    let utilization = info["utilization"]
        .as_f64()
        .filter(|v| v.is_finite() && (0.0..=1.0).contains(v));
    quota.windows.retain(|w| w.label != label);
    quota.windows.push(QuotaWindow {
        label: label.into(),
        remaining_percent: utilization
            .map(|v| (1.0 - v) * 100.0)
            .or_else(|| previous.as_ref().and_then(|w| w.remaining_percent)),
        duration_minutes: duration,
        resets_at: info["resetsAt"]
            .as_i64()
            .and_then(|v| v.checked_mul(1000))
            .or_else(|| previous.as_ref().and_then(|w| w.resets_at)),
    });
    quota.status = if info["status"] == "rejected" {
        "limited"
    } else {
        "connected"
    }
    .into();
    if utilization.is_some() {
        quota.observed_at = Some(now());
    }
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
            super::launch::command(&engine.config.claude_command)?
                .args(&engine.config.claude_args)
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
        .find(|q| q.id == engine.quota_id("Claude"))
        .unwrap_or(Quota {
            id: engine.quota_id("Claude"),
            provider: Provider::Claude,
            provider_id: engine.provider_id(),
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
            engine.store.upsert_quota(quota.clone())?;
            if engine.provider.is_none() {
                return Ok(json!({"status":"connected","logged_in":true}));
            }
            match super::claude_usage::refresh(engine).await {
                Ok(()) => Ok(json!({"status":"connected","logged_in":true,"quota":"known"})),
                Err(error) => {
                    quota.status = "error".into();
                    quota.reason = Some(error.to_string());
                    engine.store.upsert_quota(quota)?;
                    Ok(
                        json!({"status":"connected","logged_in":true,"quota":"unavailable","reason":error.to_string()}),
                    )
                }
            }
        }
        Err(e) => {
            quota.status = "error".into();
            quota.reason = Some(e.to_string());
            engine.store.upsert_quota(quota)?;
            Err(e)
        }
    }
}

pub fn commands(value: &Value) -> Vec<SlashCommand> {
    let mut result = Vec::new();
    if let Some(items) = value.as_array() {
        for item in items {
            let Some(name) = item.as_str().or_else(|| item["name"].as_str()) else {
                continue;
            };
            let name = name.trim_start_matches('/');
            if name.is_empty() || result.iter().any(|c: &SlashCommand| c.name == name) {
                continue;
            }
            result.push(SlashCommand {
                name: name.into(),
                description: item["description"].as_str().unwrap_or("").into(),
                argument_hint: item["argumentHint"]
                    .as_str()
                    .or_else(|| item["argument_hint"].as_str())
                    .unwrap_or("")
                    .into(),
            });
        }
    }
    result
}
