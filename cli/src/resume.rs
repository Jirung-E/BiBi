use anyhow::{Context, Result, bail};
use bibi_core::Store;
use bibi_server::{catalog, config::ServiceConfig, runtime::Engine, terminal};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

pub async fn run(
    data: PathBuf,
    query: &str,
    id: Option<&str>,
    list: bool,
    json: bool,
    confirmed: bool,
) -> Result<i32> {
    let config = ServiceConfig::new(data);
    config.prepare()?;
    let store = Store::open(config.data_dir.join("bibi.sqlite3"))?;
    let engine = Engine::new(store, config);
    let mut query = query.to_string();
    let entry = loop {
        let result = catalog::list(&engine, &query, None, None)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(0);
        }
        for warning in &result.warnings {
            eprintln!("{warning}");
        }
        if let Some(id) = id {
            let matches = result
                .items
                .iter()
                .filter(|e| {
                    e.id == id
                        || e.run_id.as_deref() == Some(id)
                        || e.native_id.as_deref() == Some(id)
                })
                .collect::<Vec<_>>();
            if matches.len() == 1 {
                break matches[0].clone();
            }
            if matches.len() > 1 {
                bail!("같은 ID의 연결이 여러 개입니다. --list --json의 고유 id를 사용하세요.");
            }
            if id.starts_with("run:") || id.starts_with("native:") {
                break catalog::find(&engine, id)?;
            }
            if query != id {
                query = id.into();
                continue;
            }
            bail!("선택한 세션을 찾지 못했습니다.");
        }
        for (i, e) in result.items.iter().enumerate() {
            println!(
                "{:>3}. {} · {} · {}\n     {}\n     {}",
                i + 1,
                display(&e.title),
                display(&e.provider_name),
                e.state,
                display(&e.workspace),
                display(&e.excerpt)
            );
        }
        if list {
            return Ok(0);
        }
        if !io::stdin().is_terminal() {
            bail!("선택에는 터미널이 필요합니다. --list --json으로 조회하거나 --id를 지정하세요.");
        }
        if result.items.is_empty() {
            eprintln!("일치하는 세션이 없습니다.");
        }
        if result.has_more {
            eprintln!("최근 100개를 표시합니다. 검색어로 범위를 좁히세요.");
        }
        eprint!("번호 또는 검색어 (빈 입력: 취소): ");
        io::stderr().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        if input.is_empty() {
            return Ok(0);
        }
        if let Ok(index) = input.parse::<usize>() {
            if let Some(entry) = index.checked_sub(1).and_then(|i| result.items.get(i)) {
                break entry.clone();
            }
            eprintln!("목록의 번호를 선택하세요.");
        } else {
            query = input.to_owned();
        }
    };
    let checked = terminal::check(&engine, &entry.id).await?;
    let mut confirmed = confirmed;
    if checked["requires_confirmation"] == true && !confirmed {
        if !io::stdin().is_terminal() {
            bail!("원래 앱에서 이 세션을 종료한 뒤 --confirmed-stopped로 확인하세요.");
        }
        eprint!("원래 앱에서 같은 세션을 종료했습니까? [y/N]: ");
        io::stderr().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            return Ok(0);
        }
        confirmed = true;
    }
    let result = terminal::prepare_local(&engine, &entry.id, confirmed).await?;
    if let Some(ticket) = result["ticket"].as_str() {
        terminal::run_ticket(&engine.config.data_dir, ticket).await
    } else {
        println!(
            "{}",
            result["message"]
                .as_str()
                .context("터미널 연결 결과 없음")?
        );
        Ok(0)
    }
}
// Session text is untrusted terminal data: strip control/ANSI sequences from the picker.
fn display(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(180).collect()
}
