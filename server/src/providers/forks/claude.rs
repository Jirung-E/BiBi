use super::*;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const LIMIT: u64 = 128 * 1024 * 1024;
pub(super) struct Transcript {
    pub root: PathBuf,
    path: PathBuf,
    native: String,
    rows: Vec<Value>,
    replacements: Vec<Value>,
}
pub(super) struct Copy {
    pub native: String,
    pub path: String,
}
fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
}
fn source_path(store: &Store, run: &Run, native: &str) -> Result<PathBuf> {
    if let Some(path) = &run.runtime.session_file {
        return Ok(PathBuf::from(path));
    }
    let root = store
        .setting::<String>(&format!("claude_root:{}", run.session_id()))?
        .map(PathBuf::from)
        .unwrap_or(super::super::claude_usage::config_directory()?);
    let mut found = vec![];
    for entry in fs::read_dir(root.join("projects"))?.take(20001) {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path().join(format!("{native}.jsonl"));
        if regular(&path) {
            found.push(path);
        }
    }
    if found.len() != 1 {
        bail!("원본 Claude 대화 파일을 찾을 수 없습니다. 세션을 다시 가져오세요.");
    }
    Ok(found.remove(0))
}
pub(super) fn load(store: &Store, run: &Run) -> Result<Transcript> {
    let native = run
        .session_key
        .as_deref()
        .context("원본 Claude ID가 없습니다.")?;
    uuid::Uuid::parse_str(native).context("잘못된 Claude 세션 ID")?;
    let path = source_path(store, run, native)?;
    if !regular(&path)
        || path.file_stem().and_then(|s| s.to_str()) != Some(native)
        || path.extension().is_none_or(|s| s != "jsonl")
    {
        bail!("원본 Claude 대화 파일의 형식 또는 ID가 다릅니다.");
    }
    let projects = path
        .parent()
        .and_then(Path::parent)
        .context("잘못된 Claude 저장 경로")?;
    if projects.file_name().is_none_or(|s| s != "projects") {
        bail!("Claude 루트 세션의 원본 파일이 아닙니다.");
    }
    let root = dunce::canonicalize(projects.parent().context("Claude 설정 경로 없음")?)?;
    let file = fs::File::open(&path)?;
    if file.metadata()?.len() > LIMIT {
        bail!("Claude 이력이 128 MiB를 초과했습니다.");
    }
    let mut bytes = vec![];
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        bail!("Claude 이력이 읽기 범위를 초과했습니다.");
    }
    let mut rows = vec![];
    let mut replacements = vec![];
    let mut workspace = false;
    for (i, line) in bytes.split(|b| *b == b'\n').enumerate() {
        if i > 50000 {
            bail!("Claude 이력이 50,000줄을 초과했습니다.");
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: Value = match super::super::claude_history::parse_record(line) {
            Ok(v) => v,
            Err(_) if !bytes.ends_with(b"\n") && bytes.ends_with(line) => break,
            Err(e) => return Err(e.into()),
        };
        if value["sessionId"].as_str().is_some_and(|id| id != native)
            || value["isSidechain"] == true
        {
            continue;
        }
        if let Some(cwd) = value["cwd"].as_str() {
            if !claude_live::same_workspace(cwd, &run.workspace) {
                bail!("Claude 원본 대화의 작업 폴더가 다릅니다.");
            }
            workspace = true;
        }
        if value["type"] == "content-replacement" {
            replacements.extend(
                value["replacements"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
        } else if matches!(
            value["type"].as_str(),
            Some("user" | "assistant" | "attachment" | "system" | "progress")
        ) && value["uuid"].is_string()
        {
            rows.push(value);
        }
    }
    if !workspace {
        bail!("Claude 원본 대화의 작업 폴더를 확인하지 못했습니다.");
    }
    // Follow the current transcript branch; abandoned alternatives are not inherited.
    let mut by_id: HashMap<String, Value> = rows
        .iter()
        .map(|v| (v["uuid"].as_str().unwrap().into(), v.clone()))
        .collect();
    let mut leaf = rows
        .iter()
        .rev()
        .find(|v| v["type"] != "progress")
        .and_then(|v| v["uuid"].as_str())
        .map(String::from);
    let mut chain = vec![];
    let mut seen = HashSet::new();
    while let Some(id) = leaf {
        if !seen.insert(id.clone()) {
            bail!("Claude 이력의 부모 관계가 순환합니다.");
        }
        let Some(row) = by_id.remove(&id) else {
            break;
        };
        leaf = row["parentUuid"].as_str().map(String::from);
        chain.push(row);
    }
    chain.reverse();
    Ok(Transcript {
        root,
        path,
        native: native.into(),
        rows: chain,
        replacements,
    })
}
fn text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.into();
    }
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| {
            if b["type"] == "text" {
                b["text"].as_str()
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
impl Transcript {
    pub fn points(&self) -> Result<Vec<Point>> {
        let hashes = revisions(&self.rows)?;
        let mut points = vec![];
        let mut pending = HashSet::new();
        for (i, row) in self.rows.iter().enumerate() {
            for block in row["message"]["content"].as_array().into_iter().flatten() {
                if block["type"] == "tool_use"
                    && let Some(id) = block["id"].as_str()
                {
                    pending.insert(id.to_string());
                }
                if block["type"] == "tool_result"
                    && let Some(id) = block["tool_use_id"].as_str()
                {
                    pending.remove(id);
                }
            }
            if row["type"] != "assistant" || !pending.is_empty() || row["isMeta"] == true {
                continue;
            }
            let excerpt = text(&row["message"]["content"]);
            if excerpt.trim().is_empty() {
                continue;
            }
            points.push(point(
                row["uuid"].as_str().unwrap().into(),
                &hashes[i],
                &excerpt,
                timestamp(row),
                i,
            ));
        }
        Ok(points)
    }
    pub fn messages(&self, end: usize) -> Vec<Message> {
        let mut messages = vec![];
        for row in self.rows[..=end].iter().filter(|v| {
            v["isMeta"] != true && matches!(v["type"].as_str(), Some("user" | "assistant"))
        }) {
            let content = &row["message"]["content"];
            let blocks = content.as_array().cloned().unwrap_or_else(|| {
                vec![json!({"type":"text","text":content.as_str().unwrap_or("")})]
            });
            for (i, block) in blocks.iter().enumerate() {
                let (role, body) = match block["type"].as_str() {
                    Some("text") => (
                        row["type"].as_str().unwrap(),
                        block["text"].as_str().unwrap_or("").to_owned(),
                    ),
                    Some("tool_use") => (
                        "tool",
                        format!(
                            "{}\n{}",
                            block["name"].as_str().unwrap_or("tool"),
                            block["input"]
                        ),
                    ),
                    Some("tool_result") => ("tool", text(&block["content"])),
                    Some("image" | "document") => (
                        row["type"].as_str().unwrap(),
                        "[원본 대화의 첨부 파일]".into(),
                    ),
                    _ => continue,
                };
                if body.is_empty() {
                    continue;
                }
                messages.push(Message {
                    id: format!("{}:{i}", row["uuid"].as_str().unwrap()),
                    run_id: String::new(),
                    role: role.into(),
                    text: body,
                    phase: None,
                    created_at: timestamp(row),
                });
            }
        }
        messages
    }
    pub fn fork(&self, end: usize, title: &str) -> Result<Copy> {
        let native = uuid::Uuid::new_v4().to_string();
        let path = self.path.with_file_name(format!("{native}.jsonl"));
        let rows = &self.rows[..=end];
        let ids: HashMap<&str, String> = rows
            .iter()
            .filter(|v| v["type"] != "progress")
            .map(|v| {
                (
                    v["uuid"].as_str().unwrap(),
                    uuid::Uuid::new_v4().to_string(),
                )
            })
            .collect();
        let by_id: HashMap<&str, &Value> = rows
            .iter()
            .map(|v| (v["uuid"].as_str().unwrap(), v))
            .collect();
        let mut bytes = vec![];
        for (i, row) in rows
            .iter()
            .enumerate()
            .filter(|(_, v)| v["type"] != "progress")
        {
            let old = row["uuid"].as_str().unwrap();
            let mut value = row.clone();
            let mut parent = row["parentUuid"].as_str();
            let mut seen = HashSet::new();
            while let Some(id) = parent {
                if !seen.insert(id) {
                    bail!("Claude 이력의 부모 관계가 순환합니다.");
                }
                if ids.contains_key(id) {
                    break;
                }
                parent = by_id.get(id).and_then(|v| v["parentUuid"].as_str());
            }
            value["uuid"] = json!(ids[old]);
            value["parentUuid"] = json!(parent.and_then(|id| ids.get(id)));
            value["logicalParentUuid"] =
                json!(row["logicalParentUuid"].as_str().and_then(|id| ids.get(id)));
            value["sessionId"] = json!(native);
            value["isSidechain"] = json!(false);
            value["forkedFrom"] = json!({"sessionId":self.native,"messageUuid":old});
            if i == end {
                value["timestamp"] = json!(chrono::Utc::now().to_rfc3339());
            }
            for key in ["teamName", "agentName", "slug", "sourceToolAssistantUUID"] {
                value.as_object_mut().unwrap().remove(key);
            }
            serde_json::to_writer(&mut bytes, &value)?;
            bytes.push(b'\n');
        }
        if !self.replacements.is_empty() {
            serde_json::to_writer(
                &mut bytes,
                &json!({"type":"content-replacement","sessionId":native,"replacements":self.replacements,"uuid":uuid::Uuid::new_v4().to_string()}),
            )?;
            bytes.push(b'\n');
        }
        // No UUID on title metadata: older CLI/import readers use message UUIDs
        // to locate the conversation leaf.
        serde_json::to_writer(
            &mut bytes,
            &json!({"type":"custom-title","sessionId":native,"customTitle":title}),
        )?;
        bytes.push(b'\n');
        let mut output =
            tempfile::NamedTempFile::new_in(self.path.parent().context("Claude 저장 폴더 없음")?)?;
        output.write_all(&bytes)?;
        output.as_file().sync_all()?;
        output
            .persist_noclobber(&path)
            .map_err(|error| error.error)?;
        Ok(Copy {
            native,
            path: path.to_string_lossy().into_owned(),
        })
    }
}
fn timestamp(row: &Value) -> i64 {
    row["timestamp"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
        .unwrap_or(0)
}
