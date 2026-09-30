//! Read-only Claude Code JSONL discovery. Listing never starts/resumes a model.
use super::{claude_usage, imports};
use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};
const MAX_TRANSCRIPT: u64 = 128 * 1024 * 1024;

pub fn discover(engine: &Engine, project_key: &str) -> Result<Value> {
    discover_in(engine, project_key, &claude_usage::config_directory()?)
}
fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
}
fn same_workspace(recorded: &str, workspace: &str) -> bool {
    let normalize = |path: &str| fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
    normalize(recorded) == normalize(workspace)
}
fn entries(path: &Path) -> Result<Vec<PathBuf>> {
    match fs::read_dir(path) {
        Ok(entries) => entries.take(20001).map(|entry| Ok(entry?.path())).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}
pub fn discover_in(engine: &Engine, project_key: &str, root: &Path) -> Result<Value> {
    let project = engine.store.project(project_key)?;
    let mut files = Vec::new();
    for directory in entries(&root.join("projects"))? {
        if !fs::symlink_metadata(&directory)
            .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        {
            continue;
        }
        for path in entries(&directory)? {
            if path.extension().is_some_and(|e| e == "jsonl")
                && regular(&path)
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
            {
                files.push(path);
                if files.len() > 20000 {
                    bail!("저장된 이력이 조회 범위 20,000개를 초과했습니다.");
                }
            }
        }
    }
    files.sort_by_cached_key(|path| {
        std::cmp::Reverse(fs::metadata(path).and_then(|m| m.modified()).ok())
    });
    let snapshot = engine.store.snapshot()?;
    let managed: HashSet<_> = snapshot
        .runs
        .iter()
        .chain(&snapshot.removed_sessions)
        .filter(|r| r.provider == Provider::Claude && r.origin == Origin::Managed)
        .filter_map(|r| r.session_key.as_deref())
        .collect();
    let mut imported = 0;
    let mut errors = vec![];
    let mut budget = 256 * 1024 * 1024_u64;
    for file in files {
        let native = file
            .file_stem()
            .and_then(|s| s.to_str())
            .context("세션 ID 없음")?;
        if managed.contains(native) {
            continue;
        }
        if imported >= 2000 || budget == 0 {
            errors.push(json!({"session":native,"error":"한 번의 조회 범위를 초과했습니다."}));
            break;
        }
        match belongs_to_project(&file, native, &project.workspace, &mut budget) {
            Ok(true) => (),
            Ok(false) => continue,
            Err(error) => {
                errors.push(json!({"session":native,"error":error.to_string()}));
                continue;
            }
        }
        let attempt = (|| -> Result<()> {
            let history = load(&file, native, &mut budget)?;
            let mut parent = make_run(engine, &project, native, &file, &history, None)?;
            let parent_messages = messages(&parent, &history);
            if parent_messages.is_empty() {
                return Ok(());
            }
            parent = engine
                .store
                .replace_external_history(parent, parent_messages)?;
            imported += 1;
            let folder = file.with_extension("").join("subagents");
            let children = entries(&folder)?;
            if children.len() > 200 {
                errors.push(
                    json!({"session":native,"error":"서브에이전트 이력 200개까지만 가져왔습니다."}),
                );
            }
            for child in children.into_iter().take(200) {
                if !regular(&child) || child.extension().is_none_or(|e| e != "jsonl") {
                    continue;
                }
                let Some(agent) = child
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix("agent-"))
                else {
                    continue;
                };
                if agent.is_empty()
                    || !agent
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    continue;
                }
                let loaded = (|| -> Result<()> {
                    let history = load(&child, native, &mut budget)?;
                    let run = make_run(engine, &project, agent, &child, &history, Some(&parent))?;
                    let messages = messages(&run, &history);
                    if !messages.is_empty() {
                        engine.store.replace_external_history(run, messages)?;
                    }
                    Ok(())
                })();
                if let Err(error) = loaded {
                    errors.push(
                        json!({"session":format!("{native}/{agent}"),"error":error.to_string()}),
                    );
                }
            }
            Ok(())
        })();
        if let Err(error) = attempt {
            errors.push(json!({"session":native,"error":error.to_string()}));
        }
    }
    Ok(imports::result(&project, imported, errors))
}
// A first user record can contain a large prompt before its cwd field. Read
// whole bounded JSONL records, not a prefix that silently misses those sessions.
fn belongs_to_project(
    path: &Path,
    native: &str,
    workspace: &str,
    budget: &mut u64,
) -> Result<bool> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file.take(MAX_TRANSCRIPT.min(*budget)));
    let mut line = Vec::new();
    for _ in 0..50000 {
        line.clear();
        let length = reader.read_until(b'\n', &mut line)?;
        *budget = budget.saturating_sub(length as u64);
        if length == 0 {
            break;
        }
        if let Ok(value) = parse_record(&line)
            && value["sessionId"] == native
            && let Some(cwd) = value["cwd"].as_str()
        {
            return Ok(same_workspace(cwd, workspace));
        }
    }
    Ok(false)
}
// JS JSON.stringify can persist a lone UTF-16 surrogate (for example a
// partially streamed emoji). Replace only unpaired escapes with U+FFFD, as a
// browser renders them; keep valid pairs and literal backslash sequences intact.
fn parse_record(bytes: &[u8]) -> serde_json::Result<Value> {
    let original = serde_json::from_slice(bytes);
    if original.is_ok() {
        return original;
    }
    fn unit(bytes: &[u8]) -> Option<u16> {
        if bytes.len() < 6 || &bytes[..2] != br"\u" {
            return None;
        }
        u16::from_str_radix(std::str::from_utf8(&bytes[2..6]).ok()?, 16).ok()
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut quoted = false;
    let mut changed = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            quoted = !quoted;
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if quoted && bytes[i] == b'\\' {
            if let Some(value) = unit(&bytes[i..])
                && (0xd800..=0xdfff).contains(&value)
            {
                if value <= 0xdbff
                    && unit(&bytes[i + 6..]).is_some_and(|next| (0xdc00..=0xdfff).contains(&next))
                {
                    out.extend_from_slice(&bytes[i..i + 12]);
                    i += 12;
                } else {
                    out.extend_from_slice(b"\\uFFFD");
                    i += 6;
                    changed = true;
                }
                continue;
            }
            let end = (i + 2).min(bytes.len());
            out.extend_from_slice(&bytes[i..end]);
            i = end;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    if changed {
        serde_json::from_slice(&out)
    } else {
        original
    }
}
struct History {
    rows: Vec<Value>,
    title: Option<String>,
}
fn load(path: &Path, native: &str, budget: &mut u64) -> Result<History> {
    let file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_TRANSCRIPT || size > *budget {
        bail!("이력 크기가 읽기 한도를 초과했습니다. (세션 128 MiB / 조회 256 MiB)");
    }
    *budget -= size;
    let mut bytes = Vec::new();
    file.take(size + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > size {
        bail!("이력이 갱신 중입니다. 다시 가져오세요.");
    }
    let mut rows = HashMap::<String, Value>::new();
    let mut leaf = None;
    let mut title = None;
    for (i, line) in bytes.split(|b| *b == b'\n').enumerate() {
        if i > 50000 {
            bail!("세션 이력이 50,000줄을 초과했습니다.");
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: Value = match parse_record(line) {
            Ok(v) => v,
            Err(_) if !bytes.ends_with(b"\n") && bytes.ends_with(line) => break, // a concurrent partial final line
            Err(error) => bail!(
                "저장된 JSONL 이력의 {}번째 줄을 읽을 수 없습니다: {error}",
                i + 1
            ),
        };
        if value["sessionId"].as_str().is_some_and(|s| s != native) {
            continue;
        }
        if value["type"] == "custom-title" {
            title = value["customTitle"].as_str().map(String::from);
        }
        if let Some(id) = value["uuid"].as_str() {
            let id = id.to_owned();
            // Progress records can have UUIDs but are not part of the conversation chain.
            if value["type"] == "progress" {
                continue;
            }
            leaf = Some(id.clone());
            rows.insert(id, value);
        }
    }
    let mut chain = vec![];
    let mut seen = HashSet::new();
    while let Some(id) = leaf {
        if !seen.insert(id.clone()) {
            bail!("대화 이력의 부모 관계에 순환이 있습니다.");
        }
        let Some(value) = rows.remove(&id) else {
            break;
        };
        leaf = value["parentUuid"].as_str().map(String::from);
        chain.push(value);
    }
    chain.reverse();
    Ok(History { rows: chain, title })
}
fn timestamp(value: &Value) -> Option<i64> {
    value["timestamp"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp_millis())
}
fn text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.into();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
fn make_run(
    engine: &Engine,
    project: &Project,
    native: &str,
    path: &Path,
    history: &History,
    parent: Option<&Run>,
) -> Result<Run> {
    let title = history
        .title
        .clone()
        .or_else(|| {
            history
                .rows
                .iter()
                .find(|v| v["type"] == "user")
                .map(|v| text(&v["message"]["content"]))
        })
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Claude Code 외부 세션".into());
    let created = history.rows.iter().find_map(timestamp).unwrap_or_else(now);
    let model = history
        .rows
        .iter()
        .rev()
        .filter_map(|v| v["message"]["model"].as_str())
        .find(|s| !s.starts_with('<'))
        .unwrap_or("");
    let identity = parent
        .map(|p| format!("{}/{native}", p.session_key.as_deref().unwrap_or(&p.id)))
        .unwrap_or_else(|| native.into());
    let mut run = imports::run(engine, project, &identity, &title, model, created)?;
    run.session_key = Some(native.into());
    run.updated_at = history
        .rows
        .iter()
        .rev()
        .find_map(timestamp)
        .unwrap_or(created);
    run.runtime.session_file = Some(path.to_string_lossy().into());
    run.observation_source = "claude/local-history".into();
    run.context.references.push(Evidence {
        text: "Claude Code 저장된 대화".into(),
        source: format!("claude:{identity}"),
        revision: run.updated_at.to_string(),
    });
    let answer = messages(&run, history)
        .into_iter()
        .rev()
        .find(|m| m.role == "assistant");
    run.context.previous_answer_excerpt =
        answer.as_ref().map(|m| m.text.chars().take(6000).collect());
    run.context.excerpt_truncated = answer.is_some_and(|m| m.text.chars().count() > 6000);
    if let Some(parent) = parent {
        run.agent_kind = "subagent".into();
        run.role = "외부 서브에이전트".into();
        run.parent_run_id = Some(parent.id.clone());
        run.parent_session_id = Some(parent.session_id().into());
        run.work_id = parent.work_id.clone();
        run.conversation_id = parent.conversation_id.clone();
        run.context.work_id = run.work_id.clone();
        run.context.conversation_id = run.conversation_id.clone();
        run.context.reply_to = format!("bibi://inbox/{}", run.conversation_id);
    }
    Ok(run)
}
fn messages(run: &Run, history: &History) -> Vec<Message> {
    let mut messages = vec![];
    for row in &history.rows {
        let role = match row["type"].as_str() {
            Some("user") => "user",
            Some("assistant") => "assistant",
            _ => continue,
        };
        let content = &row["message"]["content"];
        let blocks = content
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![json!({"type":"text","text":content.as_str().unwrap_or("")})]);
        for (i, block) in blocks.into_iter().enumerate() {
            let (role, body) = match block["type"].as_str() {
                Some("text") => (role, block["text"].as_str().unwrap_or("").to_owned()),
                Some("tool_use") => (
                    "tool",
                    format!(
                        "{}\n{}",
                        block["name"].as_str().unwrap_or("tool"),
                        block["input"]
                    ),
                ),
                Some("tool_result") => ("tool", text(&block["content"])),
                Some("image" | "document") => (role, "[원본 대화의 첨부 파일]".into()),
                _ => continue, // Do not expose private thinking/signatures.
            };
            if body.is_empty() {
                continue;
            }
            messages.push(Message {
                id: format!(
                    "{}:{}:{i}",
                    run.id,
                    row["uuid"].as_str().unwrap_or("unknown")
                ),
                run_id: run.id.clone(),
                role: role.into(),
                text: body,
                created_at: timestamp(row).unwrap_or(run.created_at),
                phase: None,
            });
        }
    }
    messages
}
