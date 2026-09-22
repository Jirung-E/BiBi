use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio::process::Command;

pub fn definitions() -> Value {
    json!([
        {"type":"function","function":{"name":"read_file","description":"Read a UTF-8 file inside this registered workspace, maximum 64 KiB.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"list_files","description":"List one directory inside this registered workspace, at most 200 entries.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"guild_read","description":"Read the project's openguild rules, library or quests through its public CLI.","parameters":{"type":"object","properties":{"section":{"type":"string","enum":["rule","library","quest"]},"id":{"type":"string"}},"required":["section"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"guild_record","description":"Append a sourced quest comment or create a new reference document in openguild. All participants may record results directly.","parameters":{"type":"object","properties":{"kind":{"type":"string","enum":["comment","library"]},"quest_id":{"type":"string"},"title":{"type":"string"},"body":{"type":"string"}},"required":["kind","body"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"consult","description":"Ask a read-only expert using the SAME provider/model on this work. At most two experts, no recursive delegation. Returns a durable request receipt.","parameters":{"type":"object","properties":{"question":{"type":"string"},"role":{"type":"string"}},"required":["question","role"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"inbox","description":"Read this work's persistent results. Optionally wait up to 30 seconds for a request without model polling.","parameters":{"type":"object","properties":{"request_id":{"type":"string"},"wait":{"type":"boolean"}},"additionalProperties":false}}},
        {"type":"function","function":{"name":"report","description":"Record this run's work progress, independently of runtime observation.","parameters":{"type":"object","properties":{"phase":{"type":"string"},"summary":{"type":"string"},"wait_reason":{"type":"string"},"next_action":{"type":"string"}},"required":["phase","summary","next_action"],"additionalProperties":false}}}
    ])
}
pub fn definitions_for(run: &Run) -> Value {
    let mut tools = definitions();
    if run.role.starts_with("전문가:") || run.agent_kind == "subagent" {
        tools
            .as_array_mut()
            .unwrap()
            .retain(|tool| tool["function"]["name"] != "consult");
    }
    tools
}
pub fn codex_definitions(run: &Run) -> Value {
    Value::Array(definitions_for(run).as_array().unwrap().iter().map(|tool|{
        let function=&tool["function"];
        json!({"type":"function","name":format!("bibi_{}",function["name"].as_str().unwrap()),"description":function["description"],"inputSchema":function["parameters"]})
    }).collect())
}
fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .with_context(|| format!("{key} is required"))
}
fn workspace_path(run: &Run, path: &str) -> Result<PathBuf> {
    let base = PathBuf::from(&run.workspace).canonicalize()?;
    let candidate = base.join(path).canonicalize()?;
    if !candidate.starts_with(&base) {
        bail!("Path is outside this workspace.");
    }
    Ok(candidate)
}
pub async fn execute(
    engine: &Engine,
    run: &Run,
    name: &str,
    args: &Value,
    consults: &mut u8,
) -> Result<Value> {
    match name {
        "read_file" => {
            let path = workspace_path(run, string(args, "path")?)?;
            if !path.is_file() || std::fs::metadata(&path)?.len() > 65_536 {
                bail!("File must be a UTF-8 file under 64 KiB.");
            }
            let text = tokio::fs::read_to_string(&path).await?;
            Ok(json!({"path":path,"text":text}))
        }
        "list_files" => {
            let path = workspace_path(run, string(args, "path")?)?;
            let mut entries = tokio::fs::read_dir(&path).await?;
            let mut found = Vec::new();
            let mut truncated = false;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().into_owned();
                if [".git", "node_modules", "target"].contains(&name.as_str()) {
                    continue;
                }
                if found.len() >= 200 {
                    truncated = true;
                    break;
                }
                found.push(json!({"name":name,"directory":entry.file_type().await?.is_dir()}));
            }
            Ok(json!({"path":path,"entries":found,"truncated":truncated}))
        }
        "guild_read" | "guild_record" => guild(engine, run, name, args).await,
        "consult" => {
            if *consults >= 2 || run.role.starts_with("전문가:") || run.agent_kind == "subagent"
            {
                bail!(
                    "Expert request limit reached. Return the available evidence and unresolved question."
                );
            }
            let work = engine.store.work(&run.work_id)?;
            let receipt = engine.store.submit(Submission {
                submission_id: id("consult"),
                project_key: run.project_key.clone(),
                work_id: Some(run.work_id.clone()),
                title: None,
                question: string(args, "question")?.into(),
                provider: run.provider.clone(),
                model: run.model.clone(),
                host_id: run.host_id.clone(),
                role: format!("전문가: {}", string(args, "role")?),
                mode: SubmitMode::Fresh,
                target_run_id: Some(run.id.clone()),
                expected_turn_id: None,
                expected_context_revision: Some(work.context_revision),
                read_only: true,
            })?;
            *consults += 1;
            Ok(serde_json::to_value(receipt)?)
        }
        "inbox" => {
            let request = args["request_id"].as_str();
            let waiting = args["wait"].as_bool() == Some(true);
            if waiting && request == Some(run.request_id.as_str()) {
                bail!(
                    "A run cannot wait for its own result. Use the expert request_id returned by consult."
                );
            }
            if let Some(request) = request {
                let requested = engine.store.work_runs(&run.work_id)?.into_iter()
                    .find(|r| r.request_id == request)
                    .context("Unknown request in this work. Use the exact request_id returned by consult.")?;
                if waiting
                    && run.parent_run_id.as_ref() == Some(&requested.id)
                    && !requested.state.terminal()
                {
                    bail!(
                        "An expert cannot wait for its active parent. Answer the assigned current request."
                    );
                }
            }
            let deadline =
                tokio::time::Instant::now() + Duration::from_secs(if waiting { 30 } else { 0 });
            if waiting {
                engine.store.observe(
                    &run.id,
                    RunState::WaitingExpert,
                    "전문가 회신 대기",
                    request.map(String::from),
                )?;
            }
            let result = loop {
                let entries = engine
                    .store
                    .inbox(Some(&run.conversation_id))?
                    .into_iter()
                    .filter(|e| request.is_none_or(|id| id == e.request_id))
                    .collect::<Vec<_>>();
                let statuses=engine.store.work_runs(&run.work_id)?.into_iter().filter(|r|r.conversation_id==run.conversation_id&&request.is_none_or(|id|id==r.request_id)).map(|r|json!({"run_id":r.id,"request_id":r.request_id,"state":r.state,"error":r.error})).collect::<Vec<_>>();
                let failed = request.is_some()
                    && statuses.iter().any(|r| {
                        matches!(
                            r["state"].as_str(),
                            Some("failed" | "interrupted" | "uncertain")
                        )
                    });
                if !entries.is_empty() || failed || tokio::time::Instant::now() >= deadline {
                    break json!({"results":entries,"runs":statuses});
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            };
            if waiting && engine.store.run(&run.id)?.state == RunState::WaitingExpert {
                engine
                    .store
                    .observe(&run.id, RunState::Running, "진행 중", None)?;
            }
            Ok(result)
        }
        "report" => {
            let current = engine.store.run(&run.id)?;
            let report = Activity {
                revision: current.activity.map(|a| a.revision + 1).unwrap_or(1),
                phase: string(args, "phase")?.into(),
                summary: string(args, "summary")?.into(),
                wait_reason: args["wait_reason"].as_str().map(String::from),
                next_action: string(args, "next_action")?.into(),
                references: vec![],
                reported_at: now(),
            };
            engine.store.report(&run.id, report)?;
            Ok(json!({"recorded":true}))
        }
        _ => bail!("Unsupported tool: {name}"),
    }
}
async fn guild(engine: &Engine, run: &Run, tool: &str, args: &Value) -> Result<Value> {
    let project = engine.store.project(&run.project_key)?;
    let path = project
        .guild_path
        .context("This project has no openguild connection.")?;
    let mut command = Command::new("openguild");
    command
        .args(["--guild", &path, "--json"])
        .kill_on_drop(true);
    let mut body_file = None;
    if tool == "guild_read" {
        let section = string(args, "section")?;
        if !["rule", "library", "quest"].contains(&section) {
            bail!("Unsupported guild section.");
        }
        command.arg(section);
        if let Some(id) = args["id"].as_str() {
            if id.starts_with('-') {
                bail!("Invalid record ID.");
            }
            command.args(["show", id]);
            if section == "quest" {
                command.arg("--full");
            }
        } else {
            command.arg("list");
        }
    } else {
        let body = string(args, "body")?;
        if body.len() > 65_536 {
            bail!("Record body exceeds 64 KiB.");
        }
        let dir = engine.config.data_dir.join("tool-inputs");
        tokio::fs::create_dir_all(&dir).await?;
        let file = dir.join(format!("{}.txt", id("guild")));
        crate::config::private_file(&file, body)?;
        match string(args, "kind")? {
            "comment" => {
                let quest = string(args, "quest_id")?;
                if quest.starts_with('-') {
                    bail!("Invalid quest ID.");
                }
                command
                    .args([
                        "quest",
                        "comment",
                        "add",
                        quest,
                        "--author",
                        &format!("BiBi / {}", run.role),
                        "--file",
                    ])
                    .arg(&file);
            }
            "library" => {
                command
                    .args([
                        "library",
                        "new",
                        "--title",
                        string(args, "title")?,
                        "--file",
                    ])
                    .arg(&file);
            }
            _ => bail!("Unsupported record operation."),
        }
        body_file = Some(file);
    }
    let output = tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .context("openguild timed out")??;
    if let Some(file) = body_file {
        let _ = tokio::fs::remove_file(file).await;
    }
    if !output.status.success() {
        bail!(
            "openguild: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2000)
                .collect::<String>()
        );
    }
    if tool == "guild_read"
        && args["section"] == "library"
        && args.get("id").is_none()
        && let Ok(Value::Array(books)) = serde_json::from_slice::<Value>(&output.stdout)
    {
        return Ok(Value::Array(books.into_iter().map(|book|json!({"book_id":book["book_id"],"title":book["title"],"updated_at":book["updated_at"]})).collect()));
    }
    if output.stdout.len() > 131_072 {
        return Ok(
            json!({"truncated":true,"text":String::from_utf8_lossy(&output.stdout).chars().take(32000).collect::<String>()}),
        );
    }
    Ok(serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| json!({"text":String::from_utf8_lossy(&output.stdout)})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_contract_does_not_offer_shell_or_file_mutation() {
        let tools = definitions();
        let names = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(names.contains(&"guild_record"));
        assert!(names.contains(&"consult"));
        assert!(!names.contains(&"shell"));
        assert!(!names.contains(&"write_file"));
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    #[tokio::test]
    async fn scoped_tools_allow_bounded_consultation_and_keep_project_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        tokio::fs::write(outside.path().join("outside"), "private")
            .await
            .unwrap();
        let store = Store::memory().unwrap();
        let engine = Engine::new(
            store.clone(),
            crate::config::ServiceConfig::new(dir.path().into()),
        );
        store
            .add_project(Project {
                id: "p".into(),
                name: "p".into(),
                workspace: dir.path().to_string_lossy().into(),
                guild_path: None,
                constraints: vec!["preserve".into()],
            })
            .unwrap();
        store
            .upsert_host(Host {
                id: "local".into(),
                name: "local".into(),
                platform: "test".into(),
                kind: "local".into(),
                connected: true,
                observed_at: now(),
                providers: vec![Provider::Mock],
                error: None,
            })
            .unwrap();
        let receipt = store
            .submit(Submission {
                submission_id: "root".into(),
                project_key: "p".into(),
                work_id: None,
                title: None,
                question: "coordinate".into(),
                provider: Provider::Mock,
                model: "mock".into(),
                host_id: "local".into(),
                role: "coordinator".into(),
                mode: SubmitMode::Fresh,
                target_run_id: None,
                expected_turn_id: None,
                expected_context_revision: None,
                read_only: false,
            })
            .unwrap();
        let run = store.claim_next("local").unwrap().unwrap();
        let mut consulted = 0;
        let args = json!({"question":"inspect","role":"DB"});
        let first = execute(&engine, &run, "consult", &args, &mut consulted)
            .await
            .unwrap();
        execute(&engine, &run, "consult", &args, &mut consulted)
            .await
            .unwrap();
        assert!(
            execute(&engine, &run, "consult", &args, &mut consulted)
                .await
                .is_err()
        );
        let child = store.run(first["run_id"].as_str().unwrap()).unwrap();
        assert!(child.read_only);
        assert!(
            !definitions_for(&child)
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["function"]["name"] == "consult")
        );
        assert!(
            definitions_for(&child)
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["function"]["name"] == "guild_record")
        );
        let before = store.run(&child.id).unwrap().state;
        for request in [&child.request_id, &run.request_id, "unknown-request"] {
            let result = tokio::time::timeout(
                Duration::from_millis(200),
                execute(
                    &engine,
                    &child,
                    "inbox",
                    &json!({"request_id":request,"wait":true}),
                    &mut 0,
                ),
            )
            .await
            .expect("invalid inbox waits must fail immediately");
            assert!(result.is_err());
        }
        assert_eq!(store.run(&child.id).unwrap().state, before);
        assert_eq!(child.work_id, receipt.work_id);
        assert_eq!(child.context.constraints, vec!["preserve"]);
        assert!(
            execute(&engine, &child, "consult", &args, &mut 0)
                .await
                .is_err()
        );
        assert!(
            execute(
                &engine,
                &child,
                "read_file",
                &json!({"path":outside.path().join("outside")}),
                &mut 0
            )
            .await
            .is_err()
        );
        assert!(
            execute(
                &engine,
                &child,
                "shell",
                &json!({"command":"ignored"}),
                &mut 0
            )
            .await
            .is_err()
        );
    }
}

#[cfg(test)]
mod live_guild_test {
    use super::*;
    #[tokio::test]
    #[ignore = "Explicit local guild integration probe; no model calls"]
    async fn public_guild_cli_allows_a_readonly_participant_to_record() {
        let path = std::env::var("BIBI_GUILD_SMOKE").expect("explicit guild path required");
        let dir = tempfile::tempdir().unwrap();
        let store = Store::memory().unwrap();
        let engine = Engine::new(
            store.clone(),
            crate::config::ServiceConfig::new(dir.path().into()),
        );
        store
            .add_project(Project {
                id: "guild-probe".into(),
                name: "guild-probe".into(),
                workspace: dir.path().to_string_lossy().into(),
                guild_path: Some(path.clone()),
                constraints: vec![],
            })
            .unwrap();
        store
            .upsert_host(Host {
                id: "local".into(),
                name: "local".into(),
                platform: "test".into(),
                kind: "local".into(),
                connected: true,
                observed_at: now(),
                providers: vec![Provider::Mock],
                error: None,
            })
            .unwrap();
        let receipt = store
            .submit(Submission {
                submission_id: "guild-probe".into(),
                project_key: "guild-probe".into(),
                work_id: None,
                title: None,
                question: "record integration proof".into(),
                provider: Provider::Mock,
                model: "mock".into(),
                host_id: "local".into(),
                role: "공통 도구 검증".into(),
                mode: SubmitMode::Fresh,
                target_run_id: None,
                expected_turn_id: None,
                expected_context_revision: None,
                read_only: true,
            })
            .unwrap();
        let run = store.run(&receipt.run_id).unwrap();
        let rules = execute(
            &engine,
            &run,
            "guild_read",
            &json!({"section":"rule"}),
            &mut 0,
        )
        .await
        .unwrap();
        assert!(rules.to_string().contains("bibi-development"));
        let marker = "BiBi 공통 업무 도구에서 읽기 전용 참여자의 공개 openguild CLI 규칙 조회와 댓글 기록을 검증했습니다. 모델 호출 없이 동일 실행 경로를 검사했으며 전담 기록자를 사용하지 않았습니다. 구현·검증 원본은 [[BOOK-021]]입니다.";
        async fn comments(path: &str) -> Value {
            let output = Command::new("openguild")
                .args([
                    "--guild", path, "--json", "quest", "comment", "show", "DEV-004", "--all",
                ])
                .output()
                .await
                .unwrap();
            assert!(output.status.success());
            serde_json::from_slice(&output.stdout).unwrap()
        }
        let contains_marker =
            |value: &Value| {
                value["entries"].as_array().unwrap().iter().any(|entry| {
                    entry["author"] == "BiBi / 공통 도구 검증" && entry["body"] == marker
                })
            };
        if !contains_marker(&comments(&path).await) {
            execute(
                &engine,
                &run,
                "guild_record",
                &json!({"kind":"comment","quest_id":"DEV-004","body":marker}),
                &mut 0,
            )
            .await
            .unwrap();
        }
        assert!(contains_marker(&comments(&path).await));
    }
}
