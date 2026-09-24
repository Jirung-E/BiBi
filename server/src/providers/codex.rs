use super::{rpc::Rpc, runtime_instructions};
use crate::runtime::{Control, Engine};
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};
use tokio::sync::mpsc;

pub async fn execute(
    engine: &Engine,
    run: Run,
    mut controls: mpsc::Receiver<Control>,
) -> Result<()> {
    let project = engine.store.project(&run.project_key)?;
    let previous = super::previous_session(engine, &run)?;
    let cached = if previous.is_some() {
        engine.codex_sessions.take(run.session_id()).await
    } else {
        None
    };
    let (mut rpc, reused) = match cached {
        Some(mut rpc) => {
            if rpc.alive() {
                (rpc, true)
            } else {
                (Rpc::connect_tools(&engine.config).await?, false)
            }
        }
        None => (Rpc::connect_tools(&engine.config).await?, false),
    };
    let mut params = json!({"cwd":run.workspace,"sandbox":if run.read_only{"read-only"}else{"workspace-write"},
        "approvalPolicy":if run.read_only{"never"}else{"on-request"},"approvalsReviewer":"user",
        "developerInstructions":runtime_instructions(&run,&project),"serviceName":"bibi"});
    params["dynamicTools"] = super::task_tools::codex_definitions(&run);
    if !run.model.trim().is_empty() {
        params["model"] = json!(run.model);
    }
    let thread = if let Some(native) = previous {
        if !reused {
            params.as_object_mut().unwrap().remove("dynamicTools");
            params.as_object_mut().unwrap().remove("serviceName");
            params["threadId"] = json!(native);
            let restored = rpc.request("thread/resume", params).await?;
            engine.store.runtime_metadata(
                &run.id,
                restored["model"].as_str(),
                None,
                restored["thread"]["path"].as_str().map(String::from),
            )?;
            if restored["thread"]["id"].as_str() != Some(&native) {
                bail!("Codex가 다른 세션을 복원했습니다.");
            }
        }
        native
    } else {
        let started = rpc.request("thread/start", params).await?;
        engine.store.runtime_metadata(
            &run.id,
            started["model"].as_str(),
            None,
            started["thread"]["path"].as_str().map(String::from),
        )?;
        started["thread"]["id"]
            .as_str()
            .context("Codex thread ID 없음")?
            .to_owned()
    };
    engine.store.runtime_started(&run.id, &thread)?;
    let prompt = super::prompt(&run)?;
    let mut turn_params = json!({"threadId":thread,"input":[{"type":"text","text":prompt}],"clientUserMessageId":run.request_id});
    if !run.model.trim().is_empty() {
        turn_params["model"] = json!(run.model);
    }
    let turn = rpc.request("turn/start", turn_params).await?;
    let turn_id = turn["turn"]["id"]
        .as_str()
        .context("Codex turn ID 없음")?
        .to_owned();
    engine.store.delivered(&run.id, &thread, &turn_id)?;
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    let mut streamed = HashSet::new();
    let mut consults = 0;
    let mut final_text = String::new();
    let mut awaiting_agents = false;
    let mut stats = UsageStats::default();
    let usage_key = format!("codex:usage:{}", run.session_id());
    let baseline: Value = engine.store.setting(&usage_key)?.unwrap_or(json!({}));
    let began = std::time::Instant::now();
    loop {
        tokio::select! {
            value=rpc.next()=>{
                let value=value?;
                let method=value["method"].as_str().unwrap_or("");
                let params=&value["params"];
                if let Some(native_id)=value.get("id").filter(|_|value.get("method").is_some()) {
                    if method=="item/tool/call" {
                        let tool_run=if params["threadId"].as_str()==Some(&thread)&&params["turnId"].as_str()==Some(&turn_id){run.clone()}else{
                            engine.store.work_runs(&run.work_id)?.into_iter().find(|r|r.agent_kind=="subagent"&&r.session_key.as_deref()==params["threadId"].as_str()).context("업무 도구의 실행 대상이 일치하지 않습니다.")?
                        };
                        let tool=params["tool"].as_str().unwrap_or("");
                        let name=tool.strip_prefix("bibi_").context("지원하지 않는 업무 도구")?;
                        let key=format!("tool:{}:{}",run.id,params["callId"].as_str().context("도구 호출 ID 없음")?);
                        let digest=serde_json::to_string(&(tool,&params["arguments"]))?;
                        let record=if let Some(record)=engine.store.setting::<Value>(&key)? {
                            if record["digest"].as_str()!=Some(&digest){bail!("같은 도구 호출 ID에 다른 내용이 도착했습니다.");}record
                        }else{
                            let result=tokio::select! {
                                result=super::task_tools::execute(engine,&tool_run,name,&params["arguments"],&mut consults)=>result,
                                control=controls.recv()=>{
                                    match control{
                                        Some(Control::Interrupt)=>{rpc.request("turn/interrupt",json!({"threadId":thread,"turnId":turn_id})).await?;},
                                        Some(Control::Respond{reply,..})=>{let _=reply.send(Err(anyhow::anyhow!("해당 승인 요청을 처리 중이지 않습니다.")));},
                                        None=>(),
                                    }
                                    Err(anyhow::anyhow!("업무 도구 호출이 중단되었습니다."))
                                }
                            };
                            let success=result.is_ok();let value=match result{Ok(v)=>v,Err(e)=>json!({"error":e.to_string()})};
                            let text=serde_json::to_string(&value)?;
                            let record=json!({"digest":digest,"response":{"success":success,"contentItems":[{"type":"inputText","text":text}]}});
                            engine.store.set_setting(&key,&record)?;
                            engine.store.add_message(&run.id,"tool",&format!("{tool}\n{}",text.chars().take(4000).collect::<String>()))?;
                            record
                        };
                        rpc.send(json!({"id":native_id,"result":record["response"]})).await?;
                    }else{handle_request(engine,&run,&mut rpc,method,native_id.clone(),params.clone()).await?;}
                    continue;
                }
                let child_event=observe_agents(engine,&run,&thread,method,params)?;
                if awaiting_agents && !active_agents(engine,&run)? {
                    engine.store.complete(&run.id,&final_text,stats)?;
                    if !engine.is_stopping(){engine.codex_sessions.put(run.session_id(),rpc).await;}
                    return Ok(());
                }
                if child_event {continue;}
                match method {
                    "item/agentMessage/delta"|"item/commandExecution/outputDelta"=>{
                        let item=params["itemId"].as_str().unwrap_or("stream");
                        let key=format!("{}:{item}",run.id);let role=if method.contains("agentMessage"){"assistant"}else{"tool"};
                        engine.store.append_output(&run.id,&key,role,params["delta"].as_str().unwrap_or(""))?;streamed.insert(key);
                    },
                    "item/started"=>{
                        if matches!(params["item"]["type"].as_str(),Some("commandExecution"|"fileChange"|"mcpToolCall")) {
                            engine.store.observe(&run.id,RunState::Running,"도구 실행 중",None)?;
                        }
                    },
                    "item/completed"=>{
                        let item=&params["item"];let key=format!("{}:{}",run.id,item["id"].as_str().unwrap_or("item"));
                        match item["type"].as_str() {
                            Some("agentMessage")=>{
                                let text=item["text"].as_str().unwrap_or("");
                                engine.store.set_message(Message{id:key,run_id:run.id.clone(),role:"assistant".into(),text:text.into(),created_at:now()})?;
                                if item["phase"].as_str()!=Some("commentary"){final_text=text.into();}
                            },
                            Some("commandExecution") if !streamed.contains(&key)=>{
                                let text=item["aggregatedOutput"].as_str().unwrap_or("");
                                if !text.is_empty(){engine.store.set_message(Message{id:key,run_id:run.id.clone(),role:"tool".into(),text:text.into(),created_at:now()})?;}
                            },
                            Some("fileChange")=>{
                                engine.store.set_message(Message{id:key,run_id:run.id.clone(),role:"tool".into(),
                                    text:format!("파일 변경\n{}",serde_json::to_string_pretty(&item["changes"])?),created_at:now()})?;
                            },
                            _=>(),
                        }
                    },
                    "thread/tokenUsage/updated"=>{
                        let total=&params["tokenUsage"]["total"];
                        let delta=|key:&str|total[key].as_u64().map(|n|n.checked_sub(baseline[key].as_u64().unwrap_or(0)).unwrap_or(n));
                        stats.input_tokens=delta("inputTokens");stats.cached_input_tokens=delta("cachedInputTokens");stats.output_tokens=delta("outputTokens");
                        engine.store.set_setting(&usage_key,total)?;engine.store.usage(&run.id,stats.clone())?;
                    },
                    "account/rateLimits/updated"=>{let _=persist_quota(engine,params);},
                    "turn/completed"=>{
                        let status=params["turn"]["status"].as_str().unwrap_or("failed");
                        stats.duration_ms=Some(began.elapsed().as_millis() as u64);engine.store.usage(&run.id,stats.clone())?;
                        if status=="completed" {
                            if final_text.is_empty(){
                                final_text=engine.store.detail(&run.id)?.messages.into_iter().rev().find(|m|m.role=="assistant").map(|m|m.text).unwrap_or_default();
                            }
                            if active_agents(engine,&run)? {
                                awaiting_agents=true;
                                engine.store.observe(&run.id,RunState::WaitingExpert,"서브에이전트 대기",None)?;
                                continue;
                            }
                            engine.store.complete(&run.id,&final_text,stats)?;
                        } else {
                            engine.store.fail(&run.id,params["turn"]["error"]["message"].as_str().unwrap_or(if status=="interrupted"{"사용자가 중단했습니다."}else{"Codex 실행 실패"}),status=="interrupted")?;
                        }
                        if !engine.is_stopping() {engine.codex_sessions.put(run.session_id(),rpc).await;}
                        return Ok(());
                    },
                    "error"=>{engine.store.add_message(&run.id,"system",params["message"].as_str().unwrap_or("Codex 오류"))?;},
                    _=>(),
                }
            },
            _=interval.tick()=>{
                for input in engine.store.pending_inputs(&run.id)? {
                    engine.store.input_state(&input.id,"sending")?;
                    match rpc.request("turn/steer",json!({"threadId":thread,"expectedTurnId":input.expected_turn_id,"input":[{"type":"text","text":input.text}]})).await {
                        Ok(_)=>{engine.store.input_state(&input.id,"delivered")?;},
                        Err(e)=>{engine.store.input_state(&input.id,"uncertain")?;engine.store.add_message(&run.id,"system",&format!("현재 입력 전달 확인 필요: {e}"))?;},
                    }
                }
            },
            control=controls.recv()=>match control {
                Some(Control::Interrupt)=>{rpc.request("turn/interrupt",json!({"threadId":thread,"turnId":turn_id})).await?;},
                Some(Control::Respond{approval_id,value,reply})=>{
                    let result=respond(engine,&run,&mut rpc,&approval_id,value).await;
                    let _=reply.send(result);
                },
                None=>bail!("실행 제어 연결이 종료되었습니다."),
            }
        }
    }
}
async fn handle_request(
    engine: &Engine,
    run: &Run,
    rpc: &mut Rpc,
    method: &str,
    native_id: Value,
    params: Value,
) -> Result<()> {
    let supported = matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
            | "item/tool/requestUserInput"
    );
    if !supported {
        rpc.send(json!({"id":native_id,"error":{"code":-32601,"message":"This request type is not supported by BiBi."}})).await?;
        engine.store.add_message(
            &run.id,
            "system",
            &format!("지원하지 않는 Codex 요청: {method}"),
        )?;
        return Ok(());
    }
    if run.read_only && method != "item/tool/requestUserInput" {
        let result = if method == "item/permissions/requestApproval" {
            json!({"permissions":{},"scope":"turn"})
        } else {
            json!({"decision":"decline"})
        };
        rpc.send(json!({"id":native_id,"result":result})).await?;
        return Ok(());
    }
    let title = if method.contains("requestUserInput") {
        "추가 입력"
    } else if method.contains("fileChange") {
        "파일 변경 승인"
    } else if method.contains("permissions") {
        "추가 권한 승인"
    } else {
        "명령 실행 승인"
    };
    engine.store.add_approval(Approval {
        id: id("approval"),
        run_id: run.id.clone(),
        native_id,
        kind: method.into(),
        title: title.into(),
        detail: params,
        state: "pending".into(),
        created_at: now(),
    })?;
    engine.store.observe(
        &run.id,
        RunState::WaitingUser,
        "사용자 입력 대기",
        Some(title.into()),
    )?;
    Ok(())
}
async fn respond(
    engine: &Engine,
    run: &Run,
    rpc: &mut Rpc,
    approval_id: &str,
    value: Value,
) -> Result<()> {
    let approval = engine.store.approval(approval_id)?;
    if approval.run_id != run.id || approval.state != "pending" {
        bail!("현재 실행의 열린 요청이 아닙니다.");
    }
    let result = if approval.kind == "item/tool/requestUserInput" {
        let answers = value["answers"]
            .as_object()
            .context("질문별 답변이 필요합니다.")?;
        for question in approval.detail["questions"]
            .as_array()
            .context("잘못된 질문 형식")?
        {
            let id = question["id"].as_str().context("질문 ID 없음")?;
            if !answers
                .get(id)
                .and_then(|a| a["answers"].as_array())
                .is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_string))
            {
                bail!("모든 질문에 텍스트 응답이 필요합니다.");
            }
        }
        json!({"answers":answers})
    } else {
        let decision = value["decision"]
            .as_str()
            .context("승인 또는 거절을 선택하세요.")?;
        if !["accept", "decline"].contains(&decision) {
            bail!("지원하지 않는 승인 응답입니다.");
        }
        if approval.kind == "item/permissions/requestApproval" {
            json!({"permissions":if decision=="accept"{approval.detail["permissions"].clone()}else{json!({})},"scope":"turn"})
        } else {
            json!({"decision":decision})
        }
    };
    engine.store.approval_state(approval_id, "sending")?;
    if let Err(error) = rpc
        .send(json!({"id":approval.native_id,"result":result}))
        .await
    {
        engine.store.approval_state(approval_id, "uncertain")?;
        return Err(error);
    }
    engine.store.approval_state(approval_id, "delivered")?;
    engine
        .store
        .observe(&run.id, RunState::Running, "응답 전달됨", None)?;
    Ok(())
}
pub async fn refresh(engine: &Engine) -> Result<Value> {
    let result = async {
        let mut rpc = Rpc::connect(&engine.config).await?;
        let data = rpc.request("account/rateLimits/read", json!({})).await?;
        persist_quota(engine, &data)?;
        Ok::<_, anyhow::Error>(json!({"status":"connected"}))
    }
    .await;
    if let Err(error) = &result {
        let previous = engine
            .store
            .snapshot()?
            .quotas
            .into_iter()
            .filter(|q| q.provider == Provider::Codex && q.provider_id == engine.provider_id())
            .collect::<Vec<_>>();
        if previous.is_empty() {
            engine.store.upsert_quota(Quota {
                id: engine.quota_id("Codex"),
                provider: Provider::Codex,
                provider_id: engine.provider_id(),
                account: "확인 불가".into(),
                host_id: "local".into(),
                model: None,
                status: "error".into(),
                windows: vec![],
                observed_at: None,
                reason: Some(error.to_string()),
            })?;
        } else {
            for mut quota in previous {
                quota.status = "error".into();
                quota.reason = Some(error.to_string());
                engine.store.upsert_quota(quota)?;
            }
        }
    }
    result
}
pub fn persist_quota(engine: &Engine, data: &Value) -> Result<()> {
    let buckets = quota_buckets(data);
    let account = data["accountId"].as_str().unwrap_or("Codex 로그인 계정");
    for (key, bucket) in buckets {
        let mut windows = Vec::new();
        for (name, fallback) in [("primary", "기본 한도"), ("secondary", "추가 한도")] {
            let window = &bucket[name];
            if window.is_null() {
                continue;
            }
            let duration = window["windowDurationMins"].as_i64();
            let label = match duration {
                Some(300) => "5시간".into(),
                Some(10080) => "주간".into(),
                Some(m) => format!("{m}분"),
                None => fallback.into(),
            };
            windows.push(QuotaWindow {
                label,
                remaining_percent: window["usedPercent"]
                    .as_f64()
                    .map(|p| (100.0 - p).clamp(0.0, 100.0)),
                duration_minutes: duration,
                resets_at: window["resetsAt"].as_i64().map(|v| v * 1000),
            });
        }
        engine.store.upsert_quota(Quota {
            id: if key == "codex" {
                engine.quota_id("Codex")
            } else {
                format!("{}:{key}", engine.quota_id("Codex"))
            },
            provider: Provider::Codex,
            provider_id: engine.provider_id(),
            account: account.into(),
            host_id: "local".into(),
            model: bucket["limitName"].as_str().map(String::from),
            status: if windows.iter().any(|w| w.remaining_percent.is_some()) {
                "known"
            } else {
                "unknown"
            }
            .into(),
            windows,
            observed_at: Some(now()),
            reason: None,
        })?;
    }
    Ok(())
}
fn quota_buckets(data: &Value) -> Vec<(String, Value)> {
    if let Some(buckets) = data["rateLimitsByLimitId"]
        .as_object()
        .filter(|m| !m.is_empty())
    {
        buckets
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    } else {
        vec![("codex".into(), data["rateLimits"].clone())]
    }
}
pub async fn discover(engine: &Engine, project_key: &str) -> Result<Value> {
    let project = engine.store.project(project_key)?;
    let mut rpc = Rpc::connect(&engine.config).await?;
    let mut cursor = Value::Null;
    let mut found = 0;
    let mut errors = Vec::new();
    for _ in 0..20 {
        let page=rpc.request("thread/list",json!({"cwd":project.workspace,"limit":100,"cursor":cursor,"modelProviders":[],"useStateDbOnly":true})).await?;
        for thread in page["data"].as_array().context("Codex 목록 형식 오류")? {
            let native = thread["id"].as_str().context("Codex thread ID 없음")?;
            if engine
                .store
                .snapshot()?
                .runs
                .iter()
                .any(|r| r.origin == Origin::Managed && r.session_key.as_deref() == Some(native))
            {
                continue;
            }
            match import(engine, &project, &mut rpc, native).await {
                Ok(_) => found += 1,
                Err(e) => errors.push(json!({"session":native,"error":e.to_string()})),
            }
        }
        cursor = page["nextCursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    Ok(
        json!({"imported":found,"errors":errors,"scope":{"workspace":project.workspace,"host":"local","max_sessions":2000,"active_control":false}}),
    )
}
async fn import(engine: &Engine, project: &Project, rpc: &mut Rpc, native: &str) -> Result<()> {
    let data = rpc
        .request(
            "thread/read",
            json!({"threadId":native,"includeTurns":false}),
        )
        .await?;
    let thread = &data["thread"];
    let mut turns = Vec::new();
    let mut cursor = Value::Null;
    for _ in 0..20 {
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
    turns.reverse();
    let run_id = format!("external_codex_{native}");
    let work_id = format!("external_work_{native}");
    let conversation_id = format!("external_conversation_{native}");
    let created = thread["createdAt"].as_i64().unwrap_or(now() / 1000) * 1000;
    let title = thread["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or(thread["preview"].as_str())
        .unwrap_or("Codex 외부 세션")
        .to_owned();
    let mut state = match thread["status"]["type"].as_str() {
        Some("active") => RunState::Running,
        Some("systemError") => RunState::Failed,
        _ => RunState::Disconnected,
    };
    if !matches!(state, RunState::Running | RunState::Failed) {
        state = match turns.last().and_then(|t| t["status"].as_str()) {
            Some("completed") => RunState::Completed,
            Some("failed") => RunState::Failed,
            Some("interrupted") => RunState::Interrupted,
            _ => RunState::Disconnected,
        };
    }
    let mut messages = Vec::new();
    let mut last_answer = None;
    for turn in &turns {
        for item in turn["items"].as_array().unwrap_or(&vec![]) {
            let (role, text) = match item["type"].as_str() {
                Some("userMessage") => (
                    "user",
                    item["content"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .filter_map(|v| v["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                Some("agentMessage") => {
                    let text = item["text"].as_str().unwrap_or("").to_owned();
                    last_answer = Some(text.clone());
                    ("assistant", text)
                }
                Some("commandExecution") => (
                    "tool",
                    item["aggregatedOutput"].as_str().unwrap_or("").to_owned(),
                ),
                _ => continue,
            };
            messages.push(Message {
                id: format!("{run_id}:{}", item["id"].as_str().unwrap_or("unknown")),
                run_id: run_id.clone(),
                role: role.into(),
                text,
                created_at: turn["startedAt"].as_i64().unwrap_or(created / 1000) * 1000,
            });
        }
    }
    let mut references = vec![Evidence {
        text: "Codex 저장된 대화".into(),
        source: format!("codex:{native}"),
        revision: thread["updatedAt"].to_string(),
    }];
    if !cursor.is_null() {
        references.push(Evidence {
            text: "오래된 이력 일부는 가져오기 범위 밖".into(),
            source: format!("codex:{native}"),
            revision: "이전 턴 있음".into(),
        });
    }
    let context = ContextPacket {
        schema_version: 1,
        project_key: project.id.clone(),
        work_id: work_id.clone(),
        conversation_id: conversation_id.clone(),
        request_id: format!("external_request_{native}"),
        parent_request_id: None,
        reply_to_response_id: None,
        context_revision: 1,
        role: "외부 실행".into(),
        question: title.clone(),
        goal: title.clone(),
        constraints: project.constraints.clone(),
        decisions: vec![],
        performed_actions: vec![],
        open_questions: vec![],
        references,
        source_runs: vec![],
        previous_answer_excerpt: last_answer.as_ref().map(|s| s.chars().take(6000).collect()),
        excerpt_truncated: last_answer.is_some_and(|s| s.chars().count() > 6000),
        workspace: project.workspace.clone(),
        reply_to: format!("bibi://inbox/{conversation_id}"),
    };
    let run = Run {
        session_id: run_id.clone(),
        continued_from: None,
        parent_session_id: None,
        agent_kind: "session".into(),
        id: run_id,
        project_key: project.id.clone(),
        work_id,
        conversation_id,
        request_id: context.request_id.clone(),
        parent_run_id: None,
        context_revision: 1,
        role: "외부 실행".into(),
        title,
        provider: Provider::Codex,
        provider_id: engine.provider_id(),
        model: thread["model"].as_str().unwrap_or("").into(),
        host_id: "local".into(),
        state,
        phase: "저장된 이력 조회".into(),
        wait_reason: None,
        observation_source: "codex/thread/read".into(),
        created_at: created,
        updated_at: thread["updatedAt"].as_i64().unwrap_or(now() / 1000) * 1000,
        observed_at: now(),
        session_key: Some(native.into()),
        turn_id: None,
        origin: Origin::External,
        workspace: project.workspace.clone(),
        read_only: false,
        capabilities: Capabilities::external(&Provider::Codex),
        context,
        stats: UsageStats::default(),
        runtime: RuntimeMetadata {
            provider_name: engine.provider.as_ref().map(|p| p.name.clone()),
            ..Default::default()
        },
        activity: None,
        error: None,
    };
    engine.store.import_external(run, messages)?;
    Ok(())
}

// Consume native collaboration events in addition to BiBi's explicit consult tool.
pub fn observe_agents(
    engine: &Engine,
    root: &Run,
    thread: &str,
    method: &str,
    params: &Value,
) -> Result<bool> {
    fn state(value: &str) -> Option<RunState> {
        Some(match value {
            "pendingInit" | "running" | "inProgress" => RunState::Running,
            "completed" => RunState::Completed,
            "errored" | "failed" => RunState::Failed,
            "interrupted" | "shutdown" => RunState::Interrupted,
            "notFound" => RunState::Disconnected,
            _ => return None,
        })
    }
    let find = |native: &str| -> Result<Option<Run>> {
        if native == thread {
            return Ok(Some(root.clone()));
        }
        Ok(engine
            .store
            .work_runs(&root.work_id)?
            .into_iter()
            .find(|r| {
                r.agent_kind == "subagent"
                    && r.session_key.as_deref() == Some(native)
                    && r.host_id == root.host_id
            }))
    };
    if method == "thread/started" {
        let t = &params["thread"];
        let source = &t["source"]["subAgent"]["thread_spawn"];
        if let Some(parent) = source["parent_thread_id"]
            .as_str()
            .map(find)
            .transpose()?
            .flatten()
        {
            let child = engine.store.observe_subagent(
                &parent.id,
                SubagentUpdate {
                    native_id: t["id"]
                        .as_str()
                        .context("서브에이전트 thread ID 없음")?
                        .into(),
                    event_id: "spawn".into(),
                    title: t["agentNickname"]
                        .as_str()
                        .or_else(|| t["agentRole"].as_str())
                        .unwrap_or("Codex 서브에이전트")
                        .into(),
                    state: Some(RunState::Running),
                    prompt: None,
                    text: None,
                    stats: None,
                    started: true,
                },
            )?;
            if let Some(model) = t["model"].as_str() {
                engine.store.runtime_metadata(
                    &child.id,
                    Some(model),
                    None,
                    t["path"].as_str().map(String::from),
                )?;
            }
        }
    }
    let item = &params["item"];
    if matches!(method, "item/started" | "item/completed")
        && item["type"] == "collabAgentToolCall"
        && let Some(parent) = item["senderThreadId"]
            .as_str()
            .map(find)
            .transpose()?
            .flatten()
    {
        let native_states = item["agentsStates"].as_object();
        let mut receivers = item["receiverThreadIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect::<Vec<_>>();
        for key in native_states.into_iter().flat_map(|s| s.keys()) {
            if !receivers.contains(key) {
                receivers.push(key.clone());
            }
        }
        for native in receivers {
            let status = &item["agentsStates"][&native];
            let tool = item["tool"].as_str().unwrap_or("");
            let assigns = matches!(
                tool,
                "spawnAgent" | "sendInput" | "sendMessage" | "followupTask"
            );
            let target_parent = if tool == "spawnAgent" {
                parent.id.clone()
            } else {
                find(&native)?
                    .and_then(|r| r.parent_run_id)
                    .unwrap_or_else(|| parent.id.clone())
            };
            engine.store.observe_subagent(
                &target_parent,
                SubagentUpdate {
                    native_id: native,
                    event_id: item["id"].as_str().unwrap_or("collab").into(),
                    title: String::new(),
                    state: state(
                        status["status"]
                            .as_str()
                            .unwrap_or(if tool == "spawnAgent" { "running" } else { "" }),
                    ),
                    prompt: if assigns {
                        item["prompt"].as_str().map(String::from)
                    } else {
                        None
                    },
                    text: status["message"].as_str().map(String::from),
                    stats: None,
                    started: matches!(
                        tool,
                        "spawnAgent" | "resumeAgent" | "followupTask" | "sendInput"
                    ) && method == "item/started",
                },
            )?;
        }
    }
    if matches!(method, "item/started" | "item/completed") && item["type"] == "subAgentActivity" {
        let parent = params["threadId"]
            .as_str()
            .map(find)
            .transpose()?
            .flatten()
            .unwrap_or_else(|| root.clone());
        let native = item["agentThreadId"]
            .as_str()
            .context("서브에이전트 ID 없음")?;
        let kind = item["kind"].as_str().unwrap_or("");
        engine.store.observe_subagent(
            &parent.id,
            SubagentUpdate {
                native_id: native.into(),
                event_id: item["id"].as_str().unwrap_or("activity").into(),
                title: item["agentPath"].as_str().unwrap_or("서브에이전트").into(),
                state: state(match kind {
                    "started" | "interacted" => "running",
                    "interrupted" => "interrupted",
                    "completed" => "completed",
                    _ => "",
                }),
                prompt: None,
                text: None,
                stats: None,
                started: kind == "started",
            },
        )?;
    }
    let Some(native) = params["threadId"].as_str().filter(|id| *id != thread) else {
        return Ok(false);
    };
    let Some(child) = find(native)? else {
        return Ok(true);
    };
    match method {
        "item/agentMessage/delta" | "item/commandExecution/outputDelta" => {
            let key = format!(
                "{}:{}",
                child.id,
                params["itemId"].as_str().unwrap_or("stream")
            );
            engine.store.append_output(
                &child.id,
                &key,
                if method.contains("agentMessage") {
                    "assistant"
                } else {
                    "tool"
                },
                params["delta"].as_str().unwrap_or(""),
            )?;
        }
        "item/completed" if item["type"] == "agentMessage" => {
            engine.store.set_message(Message {
                id: format!("{}:{}", child.id, item["id"].as_str().unwrap_or("item")),
                run_id: child.id.clone(),
                role: "assistant".into(),
                text: item["text"].as_str().unwrap_or("").into(),
                created_at: now(),
            })?;
        }
        "turn/completed" => {
            let parent = child.parent_run_id.as_deref().unwrap_or(&root.id);
            engine.store.observe_subagent(
                parent,
                SubagentUpdate {
                    native_id: native.into(),
                    event_id: params["turn"]["id"].as_str().unwrap_or("turn").into(),
                    title: String::new(),
                    state: state(params["turn"]["status"].as_str().unwrap_or("")),
                    prompt: None,
                    text: None,
                    stats: None,
                    started: false,
                },
            )?;
        }
        "thread/status/changed" if !child.state.terminal() => {
            let status = &params["status"];
            let (state, phase) = match status["type"].as_str() {
                Some("active")
                    if status["activeFlags"]
                        .as_array()
                        .is_some_and(|v| !v.is_empty()) =>
                {
                    (RunState::WaitingUser, "입력 대기")
                }
                Some("active") => (RunState::Running, "실행 중"),
                Some("systemError") => (RunState::Failed, "실패"),
                Some("notLoaded") => (RunState::Disconnected, "관측 연결 종료"),
                _ => return Ok(true),
            };
            engine.store.observe(&child.id, state, phase, None)?;
        }
        "thread/tokenUsage/updated" => {
            let v = &params["tokenUsage"]["last"];
            engine.store.usage(
                &child.id,
                UsageStats {
                    input_tokens: v["inputTokens"].as_u64(),
                    cached_input_tokens: v["cachedInputTokens"].as_u64(),
                    output_tokens: v["outputTokens"].as_u64(),
                    duration_ms: None,
                },
            )?;
        }
        _ => (),
    }
    Ok(true)
}

fn active_agents(engine: &Engine, run: &Run) -> Result<bool> {
    Ok(engine.store.detail(&run.id)?.children.iter().any(|c| {
        c.run.agent_kind == "subagent"
            && !c.run.state.terminal()
            && c.run.state != RunState::Disconnected
    }))
}
