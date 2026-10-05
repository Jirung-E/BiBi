//! Read the official interactive /usage screen. Never send a model prompt or
//! a login/permission response; no credentials or raw terminal output are saved.
use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

pub fn parse_screen(screen: &str) -> Vec<QuotaWindow> {
    let mut windows = Vec::new();
    let mut current = None;
    for line in screen.lines() {
        let lower = line.to_lowercase();
        let label = if lower.contains("current session") || lower.contains("현재 세션") {
            Some(("5시간", Some(300)))
        } else if lower.contains("week") || lower.contains("주간") || lower.contains("이번 주")
        {
            Some((
                if lower.contains("sonnet") {
                    "Sonnet 주간"
                } else if lower.contains("opus") {
                    "Opus 주간"
                } else {
                    "주간"
                },
                Some(10080),
            ))
        } else if lower.contains("spend limit") || lower.contains("지출 한도") {
            Some(("지출 한도", None))
        } else {
            None
        };
        if let Some(label) = label {
            current = Some(label);
        }
        let Some((label, duration_minutes)) = current else {
            continue;
        };
        let Some(percent) = line.find('%') else {
            continue;
        };
        if !lower.contains("used")
            && !lower.contains("remaining")
            && !lower.contains("사용")
            && !lower.contains("남음")
        {
            continue;
        }
        let digits: String = line[..percent]
            .trim_end()
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let Ok(value) = digits.parse::<f64>() else {
            continue;
        };
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            continue;
        }
        let remaining = if lower.contains("remaining") || lower.contains("남음") {
            value
        } else {
            100.0 - value
        };
        windows.push(QuotaWindow {
            label: label.into(),
            remaining_percent: Some(remaining),
            duration_minutes,
            resets_at: None,
        });
        current = None;
    }
    windows
}
fn merge_windows(windows: &mut Vec<QuotaWindow>, next: Vec<QuotaWindow>) -> Result<bool> {
    let mut changed = false;
    for value in next {
        if let Some(old) = windows.iter_mut().find(|w| w.label == value.label) {
            if serde_json::to_value(&*old)? != serde_json::to_value(&value)? {
                *old = value;
                changed = true;
            }
        } else {
            windows.push(value);
            changed = true;
        }
    }
    Ok(changed)
}
fn ready(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    lower.contains("claude code")
        && (lower.contains("for shortcuts") || lower.contains("shift+tab"))
        && screen
            .lines()
            .any(|line| line.trim_start().starts_with('❯') || line.trim_start().starts_with("> "))
}
#[derive(Default)]
struct TerminalReplies {
    pending: Vec<u8>,
    cursor_queries: usize,
}
impl vt100::Callbacks for TerminalReplies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        intermediate: Option<u8>,
        second_intermediate: Option<u8>,
        params: &[&[u16]],
        command: char,
    ) {
        // ConPTY's initial cursor query, like later CLI queries, can cross read
        // boundaries. Let the incremental VT parser recognize it, including the
        // cursor position at the query (not at the end of the whole chunk).
        if command == 'n'
            && intermediate.is_none()
            && second_intermediate.is_none()
            && params == [&[6][..]]
        {
            let (row, col) = screen.cursor_position();
            self.pending
                .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            self.cursor_queries += 1;
        }
    }
}
fn collect(command: CommandBuilder, limit: Duration) -> Result<Vec<QuotaWindow>> {
    collect_with_read_size::<8192>(command, limit)
}
fn collect_with_read_size<const READ_SIZE: usize>(
    command: CommandBuilder,
    limit: Duration,
) -> Result<Vec<QuotaWindow>> {
    let pair = native_pty_system().openpty(PtySize {
        rows: 48,
        cols: 140,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let (tx, rx) = mpsc::sync_channel(32);
    std::thread::spawn(move || {
        let mut buffer = [0u8; READ_SIZE];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let result = (|| {
        let mut screen = vt100::Parser::new_with_callbacks(48, 140, 0, TerminalReplies::default());
        let started = Instant::now();
        let mut sent = false;
        let mut bytes = 0;
        let mut changed = Instant::now();
        let mut windows = Vec::new();
        while started.elapsed() < limit {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(chunk) => {
                    bytes += chunk.len();
                    if bytes > 2 * 1024 * 1024 {
                        bail!("Claude CLI 사용량 화면이 너무 큽니다.");
                    }
                    screen.process(&chunk);
                    let replies = &mut screen.callbacks_mut().pending;
                    if !replies.is_empty() {
                        writer.write_all(replies)?;
                        writer.flush()?;
                        replies.clear();
                    }
                    let text = screen.screen().contents();
                    let lower = text.to_lowercase();
                    if lower.contains("do you trust")
                        || lower.contains("trust this folder")
                        || lower.contains("choose the text style")
                        || lower.contains("select login method")
                        || lower.contains("sign in to")
                    {
                        bail!(
                            "공식 Claude Code에서 로그인·첫 실행·프로젝트 신뢰 설정을 완료한 뒤 다시 확인하세요. 자동으로 승인하지 않았습니다."
                        );
                    }
                    if !sent && ready(&text) {
                        writer.write_all(b"/usage\r")?;
                        writer.flush()?;
                        sent = true;
                    } else if sent {
                        // ConPTY may split each clear/redraw into many reads. A blank or
                        // partially redrawn screen must not restart the stability timer.
                        if merge_windows(&mut windows, parse_screen(&text))? {
                            changed = Instant::now();
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
            if sent && !windows.is_empty() && changed.elapsed() > Duration::from_millis(600) {
                return Ok(windows);
            }
        }
        let phase = if !sent {
            "입력 대기 화면 미확인"
        } else if windows.is_empty() {
            "/usage 전송 후 수치 미수신"
        } else {
            "수치 안정화 대기"
        };
        let process = match child.try_wait()? {
            Some(status) => format!("종료 코드 {}", status.exit_code()),
            None => "실행 중".into(),
        };
        bail!(
            "Claude CLI /usage 수치를 읽지 못했습니다 ({phase}, {process}, 수신 {bytes}바이트, 커서 응답 {}회). 공식 CLI의 로그인·프로젝트 신뢰·화면 형식을 확인하세요. API 조회나 모델 질문으로 대체하지 않았습니다.",
            screen.callbacks().cursor_queries
        )
    })();
    // Every success/error path terminates only the isolated probe process.
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    drop(pair.master);
    drop(rx);
    result
}
pub async fn refresh(engine: &Engine) -> Result<Value> {
    let provider = engine.provider.as_ref().context("제공자 설정 없음")?;
    let result = async {
    let snapshot = engine.store.snapshot()?;
    let recent = snapshot
        .runs
        .iter()
        .filter(|r| r.provider_id == engine.provider_id() && r.host_id == "local")
        .max_by_key(|r| r.updated_at)
        .map(|r| r.project_key.as_str());
    let project = snapshot
        .projects
        .iter()
        .find(|p| Some(p.id.as_str()) == recent)
        .or_else(|| snapshot.projects.first())
        .context("CLI 조회에 사용할 프로젝트를 먼저 등록하세요.")?;
    // Positional/custom startup arguments could be interpreted as a model prompt.
    // The isolated probe uses the account's normal settings and login only.
    if !engine.config.claude_args.is_empty() {
        bail!(
            "CLI 화면 조회는 사용자 지정 실행 인자가 없는 Claude 제공자에서 지원합니다. 질문이나 이어가기 인자를 자동 실행하지 않습니다."
        );
    }
    let native = super::launch::command(&engine.config.claude_command)?;
    let mut cmd = CommandBuilder::new(native.as_std().get_program());
    cmd.args(native.as_std().get_args());
    for (key, value) in native.as_std().get_envs() {
        if let Some(value) = value {
            cmd.env(key, value);
        }
    }
    cmd.args([
        "--disable-slash-commands",
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "--strict-mcp-config",
        "--mcp-config",
        r#"{"mcpServers":{}}"#,
    ]);
    cmd.cwd(&project.workspace);
    cmd.env("TERM", "xterm-256color");
    tokio::task::spawn_blocking(move || collect(cmd, Duration::from_secs(20))).await?
    }.await;
    let mut quota = engine
        .store
        .quotas()?
        .into_iter()
        .find(|q| q.id == engine.quota_id("Claude"))
        .unwrap_or(Quota {
            id: engine.quota_id("Claude"),
            provider: Provider::Claude,
            provider_id: engine.provider_id(),
            account: provider.name.clone(),
            host_id: "local".into(),
            model: None,
            status: "unknown".into(),
            windows: vec![],
            observed_at: None,
            reason: None,
        });
    match result {
        Ok(windows) => {
            quota.windows = windows;
            quota.observed_at = Some(now());
            quota.status = "known".into();
            quota.reason = Some("공식 CLI /usage 화면에서 확인".into());
            engine.store.upsert_quota(quota)?;
            Ok(json!({"status":"known","source":"cli"}))
        }
        Err(error) => {
            quota.status = "error".into();
            quota.reason = Some(error.to_string());
            engine.store.upsert_quota(quota)?;
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // Covers process startup, terminal handshakes, the screen response, and the
    // same 600ms stabilization used in production. Keep all PTY fixtures on one
    // budget; a short cursor-query-only budget also timed these unrelated steps.
    const FIXTURE_TIMEOUT: Duration = Duration::from_secs(10);
    #[test]
    fn only_account_windows_not_context_or_cost_are_parsed() {
        let result = parse_screen(
            "Context remaining 95%\nCurrent session\n████ 25% used\nCurrent week (all models)\n18% used\nCurrent week (Sonnet only)\n99% remaining\n$4.00 cost",
        );
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].remaining_percent, Some(75.0));
        assert_eq!(result[1].remaining_percent, Some(82.0));
        assert_eq!(result[2].remaining_percent, Some(99.0));
        assert!(parse_screen("Current session\nUnavailable\nContext 50%").is_empty());
    }
    #[test]
    fn redraw_chunks_do_not_reset_stable_usage_or_drop_other_windows() {
        let mut windows = Vec::new();
        assert!(
            merge_windows(
                &mut windows,
                parse_screen("Current session\n27% used\nCurrent week\n62% used")
            )
            .unwrap()
        );
        for chunk in [
            "",
            "Current session",
            "Current session\n27% used",
            "Current session\n27% used\nCurrent week",
            "Current session\n27% used\nCurrent week\n62% used",
        ] {
            assert!(!merge_windows(&mut windows, parse_screen(chunk)).unwrap());
        }
        assert_eq!(windows.len(), 2);
        assert!(merge_windows(&mut windows, parse_screen("Current session\n28% used")).unwrap());
        assert_eq!(windows[0].remaining_percent, Some(72.0));
    }
    #[test]
    fn cursor_queries_survive_every_chunk_boundary_and_use_the_query_position() {
        let output = b"\x1b[4;7H\x1b[6n\x1b[10;20H\x1b[6n\x1b[H";
        for chunk_size in 1..=output.len() {
            let mut parser =
                vt100::Parser::new_with_callbacks(48, 140, 0, TerminalReplies::default());
            for chunk in output.chunks(chunk_size) {
                parser.process(chunk);
            }
            assert_eq!(
                parser.callbacks().pending,
                b"\x1b[4;7R\x1b[10;20R",
                "chunk size {chunk_size}"
            );
            assert_eq!(parser.callbacks().cursor_queries, 2);
        }
        let mut parser = vt100::Parser::new_with_callbacks(48, 140, 0, TerminalReplies::default());
        for chunk in [
            b"\x1b[6".as_slice(),
            b";9n",
            b"6n",
            b"\x1b]2;6n\x07",
            b"\x1b[?6n",
        ] {
            parser.process(chunk);
        }
        assert!(parser.callbacks().pending.is_empty());
    }
    #[test]
    fn isolated_terminal_reads_usage_without_sending_a_prompt() {
        assert_isolated_usage::<8192>();
    }
    #[test]
    fn isolated_terminal_reads_single_byte_chunks_without_sending_a_prompt() {
        assert_isolated_usage::<1>();
    }
    fn assert_isolated_usage<const READ_SIZE: usize>() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("input.txt");
        let mut cmd = CommandBuilder::new("node");
        cmd.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/usage-terminal.mjs"
        ));
        cmd.arg(&record);
        cmd.arg("redraw");
        let result = collect_with_read_size::<READ_SIZE>(cmd, FIXTURE_TIMEOUT).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].remaining_percent, Some(73.0));
        assert_eq!(result[1].remaining_percent, Some(38.0));
        assert_eq!(std::fs::read_to_string(record).unwrap(), "/usage\r");
    }
    #[test]
    fn isolated_terminal_answers_a_split_cursor_query_before_usage() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("input.txt");
        let mut cmd = CommandBuilder::new("node");
        cmd.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/usage-terminal.mjs"
        ));
        cmd.arg(&record);
        cmd.arg("cursor-query");
        // Deliberately spend most of the old 3s budget before the values arrive.
        // The original test then expired during stabilization, as on Windows CI.
        cmd.args(["2400", "200"]);
        let result = collect(cmd, FIXTURE_TIMEOUT).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].remaining_percent, Some(73.0));
        assert_eq!(result[1].remaining_percent, Some(38.0));
        assert_eq!(std::fs::read_to_string(&record).unwrap(), "/usage\r");
        assert_eq!(
            std::fs::read(record.with_extension("cursor")).unwrap(),
            b"\x1b[4;7R"
        );
    }
    #[tokio::test]
    async fn failed_cli_refresh_preserves_old_values_and_marks_them_unconfirmed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::memory().unwrap();
        store
            .add_project(Project {
                id: "p".into(),
                name: "fixture".into(),
                workspace: dir.path().to_string_lossy().into(),
                guild_path: None,
                constraints: vec![],
            })
            .unwrap();
        let provider:ProviderConfig = serde_json::from_value(json!({"id":"p","name":"fixture","host_id":"local","adapter":"claude","command":"must-not-be-spawned","args":["must-not-run"],"quota_source":"cli"})).unwrap();
        store.save_provider(provider, None).unwrap();
        let engine = Engine::new(store, crate::config::ServiceConfig::new(dir.path().into()))
            .configured("p")
            .unwrap();
        engine
            .store
            .upsert_quota(Quota {
                id: "local:p".into(),
                provider: Provider::Claude,
                provider_id: Some("p".into()),
                account: "fixture".into(),
                host_id: "local".into(),
                model: None,
                status: "known".into(),
                windows: parse_screen("Current session\n27% used"),
                observed_at: Some(123),
                reason: None,
            })
            .unwrap();
        assert!(
            refresh(&engine)
                .await
                .unwrap_err()
                .to_string()
                .contains("사용자 지정 실행 인자")
        );
        let quota = engine
            .store
            .quotas()
            .unwrap()
            .into_iter()
            .find(|q| q.id == "local:p")
            .unwrap();
        assert_eq!(quota.status, "error");
        assert_eq!(quota.windows[0].remaining_percent, Some(73.0));
        assert_eq!(quota.observed_at, Some(123));
        assert!(quota.reason.unwrap().contains("사용자 지정 실행 인자"));
    }
    #[test]
    fn terminal_does_not_accept_trust_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("input.txt");
        let mut cmd = CommandBuilder::new("node");
        cmd.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/usage-terminal.mjs"
        ));
        cmd.arg(&record);
        cmd.arg("trust");
        let error = collect(cmd, FIXTURE_TIMEOUT).unwrap_err().to_string();
        assert!(error.contains("자동으로 승인하지 않았습니다"), "{error}");
        assert!(!record.exists());
    }
}
