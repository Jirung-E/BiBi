use super::*;
use std::io::{BufRead, BufReader, Read};

pub(super) fn files(root: &Path, provider: &Provider) -> Result<Vec<PathBuf>> {
    let base = root.join(if *provider == Provider::Claude {
        "projects"
    } else {
        "sessions"
    });
    let mut pending = vec![(base, 0)];
    if *provider == Provider::Codex {
        pending.push((root.join("archived_sessions"), 0));
    }
    let mut files = Vec::new();
    while let Some((dir, depth)) = pending.pop() {
        if fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let meta = entry.file_type()?;
            let path = entry.path();
            if meta.is_symlink() {
                continue;
            }
            let max_depth = if *provider == Provider::Claude { 1 } else { 3 };
            if meta.is_dir() && depth < max_depth {
                pending.push((path, depth + 1));
            } else if meta.is_file() && path.extension().is_some_and(|v| v == "jsonl") {
                files.push(path);
            }
            if files.len() + pending.len() > 20_000 {
                bail!("외부 세션 검색은 제공자별 파일 20,000개까지 지원합니다.");
            }
        }
    }
    files
        .sort_by_cached_key(|p| std::cmp::Reverse(fs::metadata(p).and_then(|m| m.modified()).ok()));
    Ok(files)
}
pub(super) fn read(path: &Path, provider: &Provider, budget: &mut u64) -> Result<Option<Record>> {
    let file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size > 128 * 1024 * 1024 || size > *budget {
        bail!("대화 읽기 한도(파일 128 MiB·조회 256 MiB)를 초과했습니다.");
    }
    *budget -= size;
    let mut reader = BufReader::new(file.take(size));
    let mut line = Vec::new();
    let mut rows = Vec::new();
    let mut native = String::new();
    let mut workspace = String::new();
    let mut model = String::new();
    let mut title = String::new();
    let mut updated = 0;
    for index in 0..50_001 {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if index == 50_000 {
            bail!("대화 레코드 50,000개 읽기 한도를 초과했습니다.");
        }
        if line.len() > 16 * 1024 * 1024 {
            bail!("대화의 한 레코드가 너무 큽니다.");
        }
        let v = match crate::providers::claude_history::parse_record(&line) {
            Ok(v) => v,
            Err(_) if !line.ends_with(b"\n") => break,
            Err(_) => bail!("대화 파일 형식을 읽을 수 없습니다."),
        };
        if let Some(ts) = v["timestamp"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        {
            updated = updated.max(ts.timestamp_millis());
        }
        if *provider == Provider::Claude {
            if v["isSidechain"] == true {
                return Ok(None);
            }
            if let Some(id) = v["sessionId"].as_str() {
                if !native.is_empty() && native != id {
                    continue;
                }
                native = id.into();
            }
            if let Some(cwd) = v["cwd"].as_str() {
                workspace = cwd.into();
            }
            if v["type"] == "custom-title" {
                title = v["customTitle"].as_str().unwrap_or("").into();
            }
            if let Some(m) = v["message"]["model"]
                .as_str()
                .filter(|m| !m.starts_with('<'))
            {
                model = m.into();
            }
            if v["uuid"].is_string() && v["type"] != "progress" {
                rows.push((
                    v["uuid"].as_str().unwrap_or("").to_string(),
                    v["parentUuid"].as_str().map(str::to_string),
                    v["type"].as_str().unwrap_or("").to_string(),
                    if matches!(v["type"].as_str(), Some("user" | "assistant")) {
                        text(&v["message"]["content"])
                    } else {
                        String::new()
                    },
                ));
            }
        } else {
            let p = &v["payload"];
            if v["type"] == "session_meta" {
                if p["source"].is_object() || p["source"].as_str() == Some("subagent") {
                    return Ok(None);
                }
                native = p["id"].as_str().unwrap_or("").into();
                workspace = p["cwd"].as_str().unwrap_or("").into();
            } else if v["type"] == "turn_context" {
                model = p["model"].as_str().unwrap_or(&model).into();
            } else if v["type"] == "response_item"
                && p["type"] == "message"
                && matches!(p["role"].as_str(), Some("user" | "assistant"))
            {
                rows.push((
                    String::new(),
                    None,
                    p["role"].as_str().unwrap().to_string(),
                    text(&p["content"]),
                ));
            }
        }
    }
    if uuid::Uuid::parse_str(&native).is_err() || workspace.is_empty() {
        return Ok(None);
    }
    if *provider == Provider::Claude && path.file_stem().and_then(|s| s.to_str()) != Some(&native) {
        return Ok(None);
    }
    if *provider == Provider::Claude && rows.last().is_some_and(|r| !r.0.is_empty()) {
        let mut parents = HashMap::new();
        for r in &rows {
            parents.insert(r.0.clone(), r.clone());
        }
        let mut next = rows.last().map(|r| r.0.clone());
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        while let Some(id) = next {
            if !seen.insert(id.clone()) {
                bail!("대화 부모 관계가 순환합니다.");
            }
            let Some(r) = parents.remove(&id) else {
                break;
            };
            next = r.1.clone();
            chain.push(r);
        }
        chain.reverse();
        rows = chain;
    }
    let user = rows
        .iter()
        .rev()
        .find(|r| r.2 == "user" && !r.3.trim().is_empty())
        .map(|r| clip(&r.3, 4000))
        .unwrap_or_default();
    let assistant = rows
        .iter()
        .rev()
        .find(|r| r.2 == "assistant" && !r.3.trim().is_empty())
        .map(|r| clip(&r.3, 4000))
        .unwrap_or_default();
    if title.is_empty() {
        title = rows
            .iter()
            .find(|r| r.2 == "user" && !r.3.is_empty())
            .map(|r| clip(&r.3, 160))
            .unwrap_or_else(|| native.clone());
    }
    let mut search = String::new();
    let mut truncated = false;
    for (_, _, _, body) in rows {
        if search.len() + body.len() > 2 * 1024 * 1024 {
            truncated = true;
            break;
        }
        search.push_str(&body);
        search.push('\n');
    }
    Ok(Some(Record {
        native_id: native,
        workspace,
        title,
        model,
        updated_at: updated,
        last_user: user,
        last_assistant: assistant,
        text: search,
        truncated,
    }))
}
fn text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.into();
    }
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            matches!(
                p["type"].as_str(),
                Some("text" | "input_text" | "output_text")
            )
        })
        .filter_map(|p| p["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

// Codex records user-edited names independently of rollout transcripts.
pub(super) fn names(root: &Path) -> Result<HashMap<String, String>> {
    let path = root.join("session_index.jsonl");
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 32 * 1024 * 1024 {
        bail!("Codex 세션 이름 색인을 읽을 수 없습니다.");
    }
    let mut names = HashMap::new();
    for line in BufReader::new(fs::File::open(path)?.take(meta.len())).lines() {
        if let Ok(value) = serde_json::from_str::<Value>(&line?)
            && let (Some(id), Some(name)) = (value["id"].as_str(), value["thread_name"].as_str())
            && uuid::Uuid::parse_str(id).is_ok()
        {
            names.insert(id.into(), clip(name, 200));
        }
    }
    Ok(names)
}
