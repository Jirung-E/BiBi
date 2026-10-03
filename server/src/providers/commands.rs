use super::rpc::Rpc;
use crate::runtime::Engine;
use anyhow::{Result, bail};
use bibi_core::*;
use serde_json::{Value, json};

pub async fn skills(engine: &Engine, workspace: &str) -> Result<Vec<Value>> {
    let mut rpc = Rpc::connect_project(&engine.config, workspace).await?;
    let result = rpc
        .request(
            "skills/list",
            json!({"cwds":[workspace],"forceReload":true}),
        )
        .await?;
    let mut out = vec![];
    for entry in result["data"].as_array().into_iter().flatten() {
        if entry["errors"].as_array().is_some_and(|v| !v.is_empty()) {
            bail!("프로젝트 스킬을 읽지 못했습니다. 제공자 설정을 확인하세요.");
        }
        out.extend(entry["skills"].as_array().into_iter().flatten().cloned());
    }
    Ok(out)
}
pub async fn catalog(
    engine: &Engine,
    project: &str,
    provider_id: &str,
    run_id: Option<&str>,
) -> Result<Value> {
    let provider = engine.store.provider(provider_id)?;
    if provider.host_id != "local" {
        return crate::peer::project_command(
            engine,
            &provider,
            project,
            json!({"type":"list_commands","run_id":run_id}),
        )
        .await;
    }
    let project = engine.store.project(project)?;
    let configured = engine.configured(provider_id)?;
    let mut entries = vec![];
    let mut errors = vec![];
    let mut add = |name: &str,
                   description: &str,
                   hint: &str,
                   source: &str,
                   supported: bool,
                   reason: &str| {
        entries.push(json!({"name":name,"description":description,"argument_hint":hint,"source":source,"supported":supported,"reason":reason}))
    };
    for (name, description) in [
        ("bibi new", "새 세션"),
        ("bibi resume", "대화 찾아 이어가기"),
        ("resume", "대화 찾아 이어가기"),
        ("bibi rename", "세션 이름 변경"),
        ("bibi usage", "사용량"),
        ("bibi extensions", "프로젝트 확장 설정"),
    ] {
        add(name, description, "", "bibi", true, "");
    }
    if matches!(provider.adapter, Provider::Codex | Provider::Claude) {
        add(
            "bibi model",
            "다음 메시지의 모델 선택",
            "모델 이름",
            "bibi",
            true,
            "",
        );
        add(
            "bibi permissions",
            "다음 메시지의 승인 모드 선택",
            "",
            "bibi",
            true,
            "",
        );
    }
    let mut valid_run = None;
    if let Some(id) = run_id {
        let run = engine.store.run(id)?;
        if run.project_key != project.id || run.provider_id.as_deref() != Some(provider_id) {
            bail!("명령 조회 대상이 변경되었습니다.");
        }
        valid_run = Some(run);
    }
    match provider.adapter {
        Provider::Codex => {
            add(
                "compact",
                "현재 대화 압축 · 모델 사용량 발생",
                "",
                "provider",
                valid_run.is_some(),
                "기존 대화에서 실행하세요.",
            );
            add(
                "review",
                "현재 세션에서 코드 검토 · 모델 사용량 발생",
                "[--branch 이름 | --commit SHA | 검토 지시]",
                "provider",
                true,
                "",
            );
            add(
                "plan",
                "계획 모드로 요청 · 모델 사용량 발생",
                "요청 내용",
                "provider",
                true,
                "",
            );
            match skills(&configured, &project.workspace).await {
                Ok(skills) => {
                    for skill in skills {
                        let name = skill["name"].as_str().unwrap_or("");
                        if !name.is_empty() {
                            add(
                                &format!("skill:{name}"),
                                skill["description"].as_str().unwrap_or("스킬"),
                                "요청 내용",
                                "skill",
                                skill["enabled"].as_bool().unwrap_or(true),
                                "비활성 스킬입니다.",
                            );
                        }
                    }
                }
                Err(_) => errors.push(
                    "Codex 스킬 목록을 읽지 못했습니다. CLI 버전·설정·호스트 연결을 확인하세요.",
                ),
            }
        }
        Provider::Claude => {
            if let Some(run) = valid_run {
                for c in run
                    .runtime
                    .commands
                    .into_iter()
                    .filter(|c| c.name != "resume")
                {
                    add(
                        &c.name,
                        &c.description,
                        &c.argument_hint,
                        "provider",
                        true,
                        "",
                    );
                }
            } else {
                errors.push("Claude 세션이 연결되면 제공자가 보고한 명령 목록이 표시됩니다.");
            }
        }
        _ => errors.push("이 제공자는 대화 API에 슬래시 명령을 제공하지 않습니다."),
    }
    for (name, description) in [("terminal-setup", "터미널 설정"), ("theme", "터미널 테마")]
    {
        add(
            name,
            description,
            "",
            "provider",
            false,
            "원본 CLI의 대화형 터미널에서 실행하세요.",
        );
    }
    Ok(json!({"entries":entries,"errors":errors}))
}
