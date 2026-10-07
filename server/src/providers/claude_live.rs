//! Observe local Claude registration files without connecting to input sockets.
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde::Deserialize;
use std::{collections::HashMap, fs, io::Read, path::Path};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Registration {
    pid: u32,
    pub session_id: String,
    pub cwd: String,
    pub started_at: i64,
    #[serde(default)]
    proc_start: Option<String>,
    #[cfg(any(windows, test))]
    #[serde(default)]
    proc_start_ft: Option<String>,
    #[serde(default)]
    pid_domain: Option<String>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    pub name: Option<String>,
}
#[derive(Clone)]
pub(crate) struct LiveSession {
    pub registration: Registration,
    pub state: RunState,
    pub phase: &'static str,
    pub verified: bool,
}
fn domain() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        std::env::consts::OS
    }
}
#[cfg(unix)]
fn process_alive(entry: &Registration) -> Option<bool> {
    if entry.pid == 0 {
        return Some(false);
    }
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &entry.pid.to_string(), "-o", "lstart=", "-o", "stat="])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
        .ok()?;
    if !output.status.success() {
        return output.stderr.is_empty().then_some(false);
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut fields = text.split_whitespace().collect::<Vec<_>>();
    let state = fields.pop()?;
    if state.starts_with('Z') {
        return Some(false);
    }
    let expected = entry.proc_start.as_ref()?;
    Some(fields.join(" ") == expected.split_whitespace().collect::<Vec<_>>().join(" "))
}
#[cfg(windows)]
fn process_alive(entry: &Registration) -> Option<bool> {
    match windows_process_start(entry.pid) {
        Ok(Some(ticks)) => windows_process_identity(ticks, entry),
        Ok(None) => Some(false),
        Err(()) => None,
    }
}
#[cfg(windows)]
fn windows_process_start(pid: u32) -> std::result::Result<Option<u64>, ()> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, GetLastError, STILL_ACTIVE},
        System::Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };
    if pid == 0 {
        return Ok(None);
    }
    // Read-only native query. No shell, process-control permission or timestamp
    // conversion through floating-point milliseconds.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return if GetLastError() == ERROR_INVALID_PARAMETER {
                Ok(None)
            } else {
                Err(())
            };
        }
        let mut code = 0;
        let mut created: FILETIME = std::mem::zeroed();
        let mut exited: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        let known = GetExitCodeProcess(handle, &mut code) != 0
            && GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) != 0;
        CloseHandle(handle);
        if !known {
            return Err(());
        }
        if code != STILL_ACTIVE as u32 {
            return Ok(None);
        }
        Ok(Some(
            ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64,
        ))
    }
}
#[cfg(any(windows, test))]
fn windows_process_identity(created: u64, entry: &Registration) -> Option<bool> {
    // Claude uses procStartFt for Windows FILETIME, not startedAt (session wall
    // time) or the legacy procStart field. Missing/legacy data cannot prove death.
    if entry.proc_start.is_some() {
        return None;
    }
    let expected = entry.proc_start_ft.as_deref()?.parse::<u64>().ok()?;
    if expected == 0 || expected > 300_000_000_000_000_000 {
        return None;
    }
    Some(created == expected)
}
fn same_pid_domain(value: &str) -> bool {
    if value == domain() {
        return true;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{
            ComputerNameDnsHostname, GetComputerNameExW,
        };
        let mut buffer = [0u16; 256];
        let mut len = buffer.len() as u32;
        // Modern Windows registrations also include the host name. A domain
        // from another PC/WSL must remain unknown even if its PID exists here.
        if unsafe { GetComputerNameExW(ComputerNameDnsHostname, buffer.as_mut_ptr(), &mut len) }
            != 0
        {
            let host = String::from_utf16_lossy(&buffer[..len as usize]);
            return value.eq_ignore_ascii_case(&format!("win32:{host}"));
        }
    }
    false
}
#[cfg(not(any(unix, windows)))]
fn process_alive(_: &Registration) -> Option<bool> {
    None
}

pub(crate) fn registrations(root: &Path) -> Result<HashMap<String, LiveSession>> {
    registrations_with(root, process_alive)
}
fn registrations_with(
    root: &Path,
    probe: impl Fn(&Registration) -> Option<bool>,
) -> Result<HashMap<String, LiveSession>> {
    let directory = match fs::read_dir(root.join("sessions")) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.into()),
    };
    let mut live = HashMap::new();
    let mut total_bytes = 0;
    for (count, entry) in directory.enumerate() {
        if count >= 2000 {
            bail!("Claude 실행 등록 정보의 조회 범위를 초과했습니다.");
        }
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "json") || !fs::symlink_metadata(&path)?.is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        total_bytes += bytes.len();
        if bytes.len() > 1024 * 1024 || total_bytes > 8 * 1024 * 1024 {
            bail!("Claude 실행 등록 정보가 너무 큽니다.");
        }
        let registration: Registration = serde_json::from_slice(&bytes)
            .context("Claude 실행 등록 정보가 갱신 중입니다. 다시 조회하세요.")?;
        if uuid::Uuid::parse_str(&registration.session_id).is_err() {
            continue;
        }
        let alive = if registration
            .pid_domain
            .as_deref()
            .is_some_and(|value| !same_pid_domain(value))
        {
            None
        } else {
            probe(&registration)
        };
        let (state, phase) = match (alive, registration.status.as_str()) {
            (Some(false), _) => (RunState::Disconnected, "외부 Claude 프로세스 종료 확인"),
            (Some(true), "busy" | "shell") => (RunState::Running, "외부 Claude 작업 중"),
            (Some(true), "idle") => (RunState::WaitingUser, "외부 Claude 입력 대기"),
            (Some(true), "waiting" | "waiting_permission" | "needs_attention") => {
                (RunState::WaitingUser, "외부 Claude 응답 대기")
            }
            _ => (RunState::Uncertain, "외부 Claude 실행 상태 확인 필요"),
        };
        let observed = LiveSession {
            registration,
            state,
            phase,
            verified: alive == Some(true),
        };
        // A stale dead registration must not hide another live/unknown owner
        // of the same native conversation, regardless of directory order.
        let priority = |v: &LiveSession| {
            if v.verified {
                2
            } else if v.state != RunState::Disconnected {
                1
            } else {
                0
            }
        };
        live.entry(observed.registration.session_id.clone())
            .and_modify(|current: &mut LiveSession| {
                if priority(&observed) > priority(current) {
                    *current = observed.clone();
                }
            })
            .or_insert(observed);
    }
    Ok(live)
}
pub(crate) fn same_workspace(a: &str, b: &str) -> bool {
    let normalize = |v: &str| fs::canonicalize(v).unwrap_or_else(|_| v.into());
    normalize(a) == normalize(b)
}
pub(crate) fn apply(run: &mut Run, live: &LiveSession, child: bool) {
    run.state = if child && live.state != RunState::Disconnected {
        RunState::Uncertain
    } else {
        live.state.clone()
    };
    run.phase = if child && live.state != RunState::Disconnected {
        "부모 실행 상태 관측 · 서브에이전트 상태 확인 필요"
    } else {
        live.phase
    }
    .into();
    run.observation_source = if live.state == RunState::Disconnected {
        "claude/local-history"
    } else {
        "claude/local-session"
    }
    .into();
    run.observed_at = now();
}
pub(crate) fn refresh(store: &Store) -> Result<()> {
    let snapshot = store.snapshot()?;
    let runs = snapshot
        .runs
        .iter()
        .chain(&snapshot.removed_sessions)
        .filter(|r| {
            r.origin == Origin::External
                && r.provider == Provider::Claude
                && r.host_id == "local"
                && r.observation_source.starts_with("claude/local-")
        })
        .collect::<Vec<_>>();
    if runs.is_empty() {
        return Ok(());
    }
    let live = registrations(&super::claude_usage::config_directory()?)?;
    reconcile(store, &runs, &snapshot.removed_sessions, &live)
}
fn reconcile(
    store: &Store,
    runs: &[&Run],
    hidden: &[Run],
    live: &HashMap<String, LiveSession>,
) -> Result<()> {
    for original in runs {
        let native = original
            .parent_run_id
            .as_ref()
            .and_then(|id| runs.iter().find(|r| r.id == *id))
            .and_then(|p| p.session_key.as_ref())
            .or(original.session_key.as_ref());
        let observed = native
            .and_then(|id| live.get(id))
            .filter(|l| same_workspace(&l.registration.cwd, &original.workspace));
        let mut run = (*original).clone();
        if let Some(observed) = observed {
            apply(&mut run, observed, original.parent_run_id.is_some());
        } else if original.observation_source == "claude/local-session" {
            run.state = RunState::Uncertain;
            run.phase = "외부 Claude 실행 등록 없음 · 종료 여부 확인 불가".into();
        } else {
            run.state = RunState::Disconnected;
            run.phase = "저장된 이력 조회".into();
            run.observation_source = "claude/local-history".into();
        }
        if run.state != original.state
            || run.phase != original.phase
            || run.observation_source != original.observation_source
        {
            run.updated_at = now();
            run.observed_at = now();
            store.import_external(run.clone(), vec![])?;
        }
        // Older versions made live external sessions eligible for cleanup.
        if original.state == RunState::Disconnected
            && original.observation_source == "claude/local-history"
            && observed.is_some_and(|v| v.verified)
            && hidden.iter().any(|h| h.session_id() == run.session_id())
        {
            store.set_session_hidden(&run.id, false)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn duplicate_dead_registrations_never_hide_a_live_or_unknown_owner() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sessions")).unwrap();
        let native = uuid::Uuid::new_v4().to_string();
        for pid in 1..=3 {
            fs::write(dir.path().join("sessions").join(format!("{pid}.json")),
                json!({"pid":pid,"sessionId":native,"cwd":"/fixture","startedAt":1,"status":"idle"}).to_string()).unwrap();
        }
        let live = registrations_with(dir.path(), |r| match r.pid {
            1 => Some(true),
            2 => None,
            _ => Some(false),
        })
        .unwrap();
        assert!(live[&native].verified);
        let unknown =
            registrations_with(dir.path(), |r| if r.pid == 2 { None } else { Some(false) })
                .unwrap();
        assert_eq!(unknown[&native].state, RunState::Uncertain);
    }
    #[test]
    fn registry_distinguishes_live_dead_and_unverifiable_processes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sessions")).unwrap();
        let ids = (0..4)
            .map(|_| uuid::Uuid::new_v4().to_string())
            .collect::<Vec<_>>();
        for (index, id) in ids.iter().enumerate() {
            fs::write(dir.path().join("sessions").join(format!("{index}.json")), json!({"pid":index+1,"sessionId":id,"cwd":"/fixture","startedAt":1,"status":if index==0 {"busy"} else {"idle"},"pidDomain":domain()}).to_string()).unwrap();
        }
        let live = registrations_with(dir.path(), |entry| match entry.pid {
            1 | 2 => Some(true),
            3 => Some(false),
            _ => None,
        })
        .unwrap();
        assert_eq!(live[&ids[0]].state, RunState::Running);
        assert_eq!(live[&ids[1]].state, RunState::WaitingUser);
        assert_eq!(live[&ids[2]].state, RunState::Disconnected);
        assert!(!live[&ids[2]].verified);
        assert_eq!(live[&ids[3]].state, RunState::Uncertain);
        assert!(!live[&ids[3]].verified);
    }

    #[test]
    fn windows_identity_uses_exact_filetime_not_session_registration_time() {
        let ticks = 134_352_036_123_456_789_u64;
        let mut entry: Registration = serde_json::from_value(json!({
            "pid":1,"sessionId":uuid::Uuid::new_v4().to_string(),"cwd":"/fixture",
            "startedAt":1,"procStartFt":ticks.to_string()
        }))
        .unwrap();
        assert_eq!(windows_process_identity(ticks, &entry), Some(true));
        assert_eq!(windows_process_identity(ticks + 1, &entry), Some(false));
        entry.proc_start_ft = None;
        assert_eq!(windows_process_identity(ticks, &entry), None);
        for invalid in [
            "not-a-time",
            "0",
            "999999999999999999999999",
            "638999999999999999",
        ] {
            entry.proc_start_ft = Some(invalid.into());
            assert_eq!(windows_process_identity(ticks, &entry), None);
        }
        entry.proc_start_ft = Some(ticks.to_string());
        entry.proc_start = Some("legacy-time".into());
        assert_eq!(windows_process_identity(ticks, &entry), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_native_probe_recognizes_live_process_and_reused_pid() {
        let ticks = windows_process_start(std::process::id()).unwrap().unwrap();
        let mut entry: Registration = serde_json::from_value(json!({
            "pid":std::process::id(),"sessionId":uuid::Uuid::new_v4().to_string(),
            "cwd":".","startedAt":1,"procStartFt":ticks.to_string()
        }))
        .unwrap();
        assert_eq!(process_alive(&entry), Some(true));
        entry.proc_start_ft = Some((ticks - 1).to_string());
        assert_eq!(process_alive(&entry), Some(false));
        entry.proc_start_ft = None;
        assert_eq!(process_alive(&entry), None);
        assert_eq!(windows_process_start(0), Ok(None));
        assert!(same_pid_domain("win32"));
        assert!(!same_pid_domain("win32:another-host-that-does-not-exist"));
    }
    #[test]
    fn live_reconciliation_repairs_cleanup_and_preserves_messages_and_explicit_hiding() {
        use crate::{config::ServiceConfig, runtime::Engine};
        let dir = tempfile::tempdir().unwrap();
        let store = Store::memory().unwrap();
        let project = Project {
            id: "p".into(),
            name: "fixture".into(),
            workspace: dir.path().to_string_lossy().into(),
            guild_path: None,
            constraints: vec![],
        };
        store.add_project(project.clone()).unwrap();
        let mut engine = Engine::new(store.clone(), ServiceConfig::new(dir.path().into()));
        engine.provider = Some(ProviderConfig {
            id: "claude".into(),
            host_id: "local".into(),
            remote_id: None,
            name: "fixture".into(),
            adapter: Provider::Claude,
            command: "must-not-run".into(),
            args: vec![],
            endpoint: String::new(),
            models: vec![],
            api_key_set: false,
            quota_source: None,
            ollama: None,
        });
        let native = uuid::Uuid::new_v4().to_string();
        let mut run =
            super::super::imports::run(&engine, &project, &native, "Saved title", "fixture", 1)
                .unwrap();
        run.observation_source = "claude/local-history".into();
        let message = Message {
            id: "message".into(),
            run_id: run.id.clone(),
            role: "assistant".into(),
            text: "Keep this answer".into(),
            phase: Some("final".into()),
            created_at: 1,
        };
        store.import_external(run.clone(), vec![message]).unwrap();
        store.set_session_hidden(&run.id, true).unwrap();
        let registration:Registration=serde_json::from_value(json!({"pid":1,"sessionId":native,"cwd":project.workspace,"startedAt":1,"status":"busy"})).unwrap();
        let live = HashMap::from([(
            native.clone(),
            LiveSession {
                registration,
                state: RunState::Running,
                phase: "외부 Claude 작업 중",
                verified: true,
            },
        )]);
        let snapshot = store.snapshot().unwrap();
        reconcile(
            &store,
            &snapshot.removed_sessions.iter().collect::<Vec<_>>(),
            &snapshot.removed_sessions,
            &live,
        )
        .unwrap();
        assert!(store.snapshot().unwrap().removed_sessions.is_empty());
        assert_eq!(store.run(&run.id).unwrap().state, RunState::Running);
        assert_eq!(
            store.detail(&run.id).unwrap().messages[0].text,
            "Keep this answer"
        );
        let result = store
            .cleanup_disconnected_sessions("p", std::slice::from_ref(&run.id))
            .unwrap();
        assert!(result.hidden_session_ids.is_empty());
        assert_eq!(result.skipped_run_ids, vec![run.id.clone()]);
        store.set_session_hidden(&run.id, true).unwrap();
        let snapshot = store.snapshot().unwrap();
        reconcile(
            &store,
            &snapshot.removed_sessions.iter().collect::<Vec<_>>(),
            &snapshot.removed_sessions,
            &live,
        )
        .unwrap();
        assert_eq!(store.snapshot().unwrap().removed_sessions.len(), 1);
        let snapshot = store.snapshot().unwrap();
        reconcile(
            &store,
            &snapshot.removed_sessions.iter().collect::<Vec<_>>(),
            &snapshot.removed_sessions,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(store.run(&run.id).unwrap().state, RunState::Uncertain);
        assert_eq!(store.snapshot().unwrap().removed_sessions.len(), 1);
        let snapshot = store.snapshot().unwrap();
        let mut dead = live.clone();
        dead.get_mut(&native).unwrap().state = RunState::Disconnected;
        dead.get_mut(&native).unwrap().verified = false;
        dead.get_mut(&native).unwrap().phase = "외부 Claude 프로세스 종료 확인";
        reconcile(
            &store,
            &snapshot.removed_sessions.iter().collect::<Vec<_>>(),
            &snapshot.removed_sessions,
            &dead,
        )
        .unwrap();
        assert_eq!(store.run(&run.id).unwrap().state, RunState::Disconnected);
        assert_eq!(
            store.detail(&run.id).unwrap().messages[0].text,
            "Keep this answer"
        );
        let snapshot = store.snapshot().unwrap();
        reconcile(
            &store,
            &snapshot.removed_sessions.iter().collect::<Vec<_>>(),
            &snapshot.removed_sessions,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(store.run(&run.id).unwrap().state, RunState::Disconnected);
    }
}
