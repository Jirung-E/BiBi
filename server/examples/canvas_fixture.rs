use bibi_core::*;
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("temporary data directory"));
    let count: usize = args.next().unwrap_or_else(|| "500".into()).parse()?;
    anyhow::ensure!(
        count == 100 || count == 500,
        "Fixture size must be 100 or 500"
    );
    anyhow::ensure!(
        !dir.join("bibi.sqlite3").exists(),
        "Refusing to alter an existing database"
    );
    let config = bibi_server::config::ServiceConfig::new(dir.clone());
    config.prepare()?;
    let store = Store::open(dir.join("bibi.sqlite3"))?;
    store.add_project(Project {
        id: "canvas-load".into(),
        name: format!("부하 검증 {count}"),
        workspace: dir.to_string_lossy().into(),
        guild_path: None,
        constraints: vec!["모의 실행만 사용".into()],
    })?;
    store.upsert_host(Host {
        id: "local".into(),
        name: "부하 검증".into(),
        platform: std::env::consts::OS.into(),
        kind: "local".into(),
        connected: true,
        observed_at: now(),
        providers: vec![Provider::Mock],
        error: None,
    })?;
    let mut parent = None;
    for i in 0..count {
        if i % 10 == 0 {
            parent = None;
        }
        let request = Submission {
            submission_id: format!("load-{i}"),
            project_key: "canvas-load".into(),
            work_id: None,
            title: Some(format!("검증 업무 {}", i / 10)),
            question: format!("검증 실행 {i}"),
            provider: Provider::Mock,
            model: "mock".into(),
            host_id: "local".into(),
            role: if i % 3 == 0 {
                "업무 조정"
            } else if i % 3 == 1 {
                "DB"
            } else {
                "검증"
            }
            .into(),
            mode: SubmitMode::Fresh,
            target_run_id: parent.clone(),
            expected_turn_id: None,
            expected_context_revision: parent.as_ref().map(|_| 1),
            read_only: true,
        };
        let receipt = store.submit(request.clone())?;
        let run = store.claim_next("local")?.unwrap();
        store.delivered(
            &run.id,
            &format!("mock-session-{i}"),
            &format!("mock-turn-{i}"),
        )?;
        let steering = Submission {
            submission_id: format!("input-{i}"),
            target_run_id: Some(run.id.clone()),
            expected_turn_id: Some(format!("mock-turn-{i}")),
            question: "검증 입력".into(),
            mode: SubmitMode::Steer,
            ..request
        };
        store.submit(steering.clone())?;
        store.input_state(&format!("input-{i}"), "sending")?;
        store.input_state(&format!("input-{i}"), "delivered")?;
        if i % 10 == 0 {
            let extra = format!("input-root-{i}");
            store.submit(Submission {
                submission_id: extra.clone(),
                ..steering
            })?;
            store.input_state(&extra, "sending")?;
            store.input_state(&extra, "delivered")?;
        }
        store.complete(&run.id, "모의 부하 데이터", UsageStats::default())?;
        parent = Some(receipt.run_id);
    }
    let snapshot = store.snapshot()?;
    println!(
        "{}",
        serde_json::json!({"data_dir":dir,"nodes":snapshot.runs.len(),"edges":snapshot.transmissions.len()})
    );
    Ok(())
}
