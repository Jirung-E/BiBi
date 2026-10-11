//! Explicit result publication. No workspace scan and no remote URL fetching.
use crate::{attachments, runtime::Engine};
use anyhow::{Context, Result, bail};
use bibi_core::*;
use cap_std::{ambient_authority, fs::Dir};
use pulldown_cmark::{Event, Parser, Tag};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
};

fn local_path(run: &Run, source: &str) -> Result<PathBuf> {
    let path = Path::new(source);
    // URLs (including file:, sandbox:, UNC and NT device paths) are not files
    // served by this host. A drive prefix is accepted only on Windows.
    if source.is_empty()
        || source.starts_with("//")
        || source.starts_with("\\\\")
        || source.chars().any(char::is_control)
        || source.contains('?')
        || source.contains('#')
        || (source.contains(':') && !cfg!(windows))
    {
        bail!("작업 폴더 안의 일반 파일 경로가 필요합니다.");
    }
    let base = dunce::canonicalize(&run.workspace)?;
    let relative = if path.is_absolute() {
        // macOS exposes /tmp as /private/tmp. Canonicalize the parent only;
        // the leaf is still checked/opened through the directory capability.
        let parent = dunce::canonicalize(path.parent().context("파일 경로가 필요합니다.")?)?;
        parent
            .join(path.file_name().context("파일 이름이 필요합니다.")?)
            .strip_prefix(&base)
            .context("작업 폴더 밖의 파일은 결과로 공개하지 않습니다.")?
            .to_owned()
    } else {
        path.to_owned()
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        || relative.to_string_lossy().contains(':')
    {
        bail!("상위 폴더·장치 경로는 결과로 공개하지 않습니다.");
    }
    Ok(relative)
}
fn read_output(run: &Run, source: &str) -> Result<(String, Vec<u8>)> {
    let relative = local_path(run, source)?;
    let dir = Dir::open_ambient_dir(&run.workspace, ambient_authority())?;
    let mut partial = PathBuf::new();
    for component in relative.components() {
        partial.push(component);
        if dir.symlink_metadata(&partial)?.file_type().is_symlink() {
            bail!("심볼릭 링크는 결과로 공개하지 않습니다.");
        }
    }
    let before = dir.symlink_metadata(&relative)?;
    if !before.is_file() || before.len() > attachments::MAX_FILE as u64 {
        bail!("8 MiB 이하의 일반 파일이 필요합니다.");
    }
    // Capability-relative open prevents a raced parent symlink from escaping
    // the workspace. Read from the opened handle, never reopen an ambient path.
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut file = dir.open_with(&relative, &options)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > attachments::MAX_FILE as u64 {
        bail!("8 MiB 이하의 일반 파일이 필요합니다.");
    }
    let mut data = Vec::new();
    (&mut file)
        .take(attachments::MAX_FILE as u64 + 1)
        .read_to_end(&mut data)?;
    let after = file.metadata()?;
    if data.len() > attachments::MAX_FILE
        || opened.len() != data.len() as u64
        || after.len() != opened.len()
        || after.modified()? != opened.modified()?
    {
        bail!("읽는 동안 결과 파일이 변경됐습니다. 저장을 마친 뒤 다시 등록하세요.");
    }
    Ok((
        relative
            .file_name()
            .context("파일 이름이 필요합니다.")?
            .to_string_lossy()
            .into(),
        data,
    ))
}
fn save(
    engine: &Engine,
    run: &Run,
    message: Option<&str>,
    name: String,
    source: Option<String>,
    data: &[u8],
) -> Result<Attachment> {
    let seed = serde_json::to_vec(&json!([
        run.id,
        name,
        source,
        format!("{:x}", Sha256::digest(data))
    ]))?;
    let id = format!("att_{}", &format!("{:x}", Sha256::digest(seed))[..32]);
    let mut meta = attachments::metadata(id, run.project_key.clone(), name, data)?;
    meta.source = source;
    Ok(engine
        .store
        .publish_attachment(&run.id, message, meta, data)?)
}
pub fn publish(engine: &Engine, run: &Run, args: &Value) -> Result<Value> {
    let (name, source, data) = match (args.get("path"), args.get("content")) {
        (Some(path), None) => {
            let path = path.as_str().context("path must be a string")?;
            let (name, data) = read_output(run, path)?;
            (name, Some(path.to_owned()), data)
        }
        (None, Some(content)) => {
            let content = content.as_str().context("content must be UTF-8 text")?;
            if content.len() > 256 * 1024 {
                bail!("직접 등록하는 텍스트는 256 KiB 이하여야 합니다.");
            }
            (
                args["name"]
                    .as_str()
                    .context("name is required with content")?
                    .into(),
                None,
                content.as_bytes().to_vec(),
            )
        }
        _ => bail!("path 또는 name과 content 중 하나를 지정하세요."),
    };
    let file = save(engine, run, None, name, source, &data)?;
    Ok(
        json!({"attachment":file,"url":format!("#bibi-attachment-{}",file.id),"message":"The file is stored and visible in this conversation. Use this URL in a Markdown link or image, not a local path."}),
    )
}
/// Called once before the completed event. Old resumed Codex threads keep their
/// original dynamic tool set, so explicit Markdown file references also work.
pub fn capture_outputs(engine: &Engine, run_id: &str) -> Result<()> {
    let detail = engine.store.detail(run_id)?;
    if detail.run.origin != Origin::Managed || detail.run.host_id != "local" {
        return Ok(());
    }
    let mut examined = std::collections::HashSet::new();
    for message in detail
        .messages
        .iter()
        .filter(|m| m.role == "assistant" && m.phase.as_deref() != Some("commentary"))
    {
        for event in Parser::new(&message.text) {
            let source = match event {
                Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                    dest_url.into_string()
                }
                _ => continue,
            };
            if source.starts_with('#')
                || source.contains("://")
                || source.starts_with("data:")
                || source.starts_with("mailto:")
                || !examined.insert(source.clone())
            {
                continue;
            }
            if examined.len() > 8 {
                engine.store.add_message(
                    run_id,
                    "system",
                    "결과 파일은 응답당 8개까지 보관합니다. 나머지는 별도로 요청하세요.",
                )?;
                break;
            }
            // A tool-published snapshot wins over a changed on-disk file.
            if detail
                .messages
                .iter()
                .flat_map(|m| &m.attachments)
                .any(|a| a.source.as_deref() == Some(&source))
            {
                continue;
            }
            let result = percent_encoding::percent_decode_str(&source)
                .decode_utf8()
                .map_err(anyhow::Error::from)
                .and_then(|decoded| read_output(&detail.run, &decoded))
                .and_then(|(name, data)| {
                    save(
                        engine,
                        &detail.run,
                        Some(&message.id),
                        name,
                        Some(source.clone()),
                        &data,
                    )
                });
            if let Err(error) = result {
                engine.store.add_message(
                    run_id,
                    "system",
                    &format!("결과 파일을 보관하지 못했습니다 ({source}): {error}"),
                )?;
            }
        }
    }
    engine.store.finish_artifacts(run_id)?;
    Ok(())
}
