use bibi_core::*;
use serde_json::json;
fn main() -> anyhow::Result<()> {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("temporary data path"));
    anyhow::ensure!(
        !path.join("bibi.sqlite3").exists(),
        "Fresh fixture directory required"
    );
    let config = bibi_server::config::ServiceConfig::new(path.clone());
    config.prepare()?;
    let s = Store::open(path.join("bibi.sqlite3"))?;
    s.add_project(Project {
        id: "ui-fixture".into(),
        name: "UI 검증".into(),
        workspace: path.to_string_lossy().into(),
        guild_path: None,
        constraints: vec!["모의 데이터만 사용".into()],
    })?;
    s.upsert_host(Host {
        id: "local".into(),
        name: "검증 서버".into(),
        platform: std::env::consts::OS.into(),
        kind: "local".into(),
        connected: true,
        observed_at: now(),
        providers: vec![Provider::Mock],
        error: None,
    })?;
    s.save_provider(serde_json::from_value(json!({"id":"fixture","name":"UI 검증 · 모의","adapter":"mock","models":["fixture-model","fixture-small"]}))?,None)?;
    let a=s.submit(serde_json::from_value(json!({"submission_id":"ui-fixture","project_key":"ui-fixture","question":"화면 검증","provider":"mock","provider_id":"fixture","model":"fixture-model","host_id":"local","role":"비서","mode":"fresh","read_only":true}))?)?;
    s.claim_next("local")?;
    s.delivered(&a.run_id, "fixture-native", "fixture-turn")?;
    let text = "# 마크다운 출력\n\n**굵은 글씨**와 `코드`입니다.\n\n- 모델 기록 저장\n- 세션 이름 변경\n\n| 기능 | 상태 |\n| --- | --- |\n| 마크다운 | 확인 |\n\n```rust\nfn main() { println!(\"BiBi\"); }\n```";
    s.add_message(&a.run_id, "assistant", text)?;
    s.observe_subagent(
        &a.run_id,
        SubagentUpdate {
            native_id: "fixture-child".into(),
            event_id: "child-output".into(),
            title: "검토 전문가".into(),
            prompt: Some("출력 확인".into()),
            text: Some("검토 완료".into()),
            state: Some(RunState::Completed),
            stats: None,
            started: true,
        },
    )?;
    s.complete(&a.run_id, text, UsageStats::default())?;
    s.rename_session(&a.run_id, "BiBi 화면 검증")?;
    println!("Fixture created in {}", path.display());
    Ok(())
}
