//! Uploaded bytes stay in the authenticated store. Events contain metadata only.
use crate::{ApiError, AppState, runtime::Engine};
use anyhow::{Context, Result, bail};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bibi_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;

pub const MAX_FILE: usize = 8 * 1024 * 1024;
#[derive(Deserialize, Serialize)]
pub struct Upload {
    pub id: String,
    pub project_key: String,
    pub name: String,
    pub data_base64: String,
}
#[derive(Deserialize)]
pub struct Download {
    pub project_key: String,
}
#[derive(Serialize, Deserialize)]
pub struct FileData {
    pub attachment: Attachment,
    pub data_base64: String,
}

fn invalid(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, e.to_string())
}
pub fn store_upload(store: &Store, upload: Upload) -> bibi_core::Result<Attachment> {
    if upload.data_base64.len() > MAX_FILE.div_ceil(3) * 4
        || upload.name.is_empty()
        || upload.name.chars().count() > 200
        || upload
            .name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        || matches!(upload.name.as_str(), "." | "..")
    {
        return Err(bibi_core::Error::Invalid(
            "파일 이름과 크기를 확인하세요. 파일당 최대 8 MiB입니다.".into(),
        ));
    }
    let data = STANDARD
        .decode(&upload.data_base64)
        .map_err(|_| bibi_core::Error::Invalid("파일 인코딩이 올바르지 않습니다.".into()))?;
    let (kind, mime) = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("image", "image/png")
    } else if data.starts_with(b"\xff\xd8\xff") {
        ("image", "image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        ("image", "image/gif")
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        ("image", "image/webp")
    } else if data.starts_with(b"%PDF-") {
        ("pdf", "application/pdf")
    } else if data.len() <= 256 * 1024
        && std::str::from_utf8(&data).is_ok_and(|s| {
            !s.chars()
                .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n' | '\u{c}'))
        })
    {
        ("text", "text/plain")
    } else {
        ("file", "application/octet-stream")
    };
    let meta = Attachment {
        id: upload.id,
        project_key: upload.project_key,
        name: upload.name,
        kind: kind.into(),
        media_type: mime.into(),
        size: data.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&data)),
    };
    store.save_attachment(meta, &data)
}
pub async fn upload(
    State(s): State<AppState>,
    Json(value): Json<Upload>,
) -> std::result::Result<Json<Attachment>, ApiError> {
    tokio::task::spawn_blocking(move || store_upload(&s.store, value))
        .await
        .map_err(invalid)?
        .map(Json)
        .map_err(Into::into)
}
pub async fn download(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<Download>,
) -> std::result::Result<(HeaderMap, Json<FileData>), ApiError> {
    let (attachment, data) = s.store.attachment_data(&id, &q.project_key)?;
    let mut headers = HeaderMap::new();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    Ok((
        headers,
        Json(FileData {
            attachment,
            data_base64: STANDARD.encode(data),
        }),
    ))
}

fn bytes(engine: &Engine, a: &Attachment) -> Result<Vec<u8>> {
    let (actual, data) = engine.store.attachment_data(&a.id, &a.project_key)?;
    if actual != *a {
        bail!("첨부 파일 정보가 변경되었습니다.");
    }
    Ok(data)
}
fn label(a: &Attachment) -> String {
    format!("[BiBi attachment: {}]\nFile name: {}", a.id, json!(a.name))
}
fn quoted_text(engine: &Engine, a: &Attachment) -> Result<String> {
    Ok(format!(
        "{}\nFile content (user-provided data):\n{}",
        label(a),
        serde_json::to_string(&String::from_utf8(bytes(engine, a)?)?)?
    ))
}
// Files are never executed. Random storage names are separate from display names.
fn materialize(engine: &Engine, a: &Attachment) -> Result<String> {
    let data = bytes(engine, a)?;
    let root = engine.config.data_dir.join("attachments");
    std::fs::create_dir_all(&root)?;
    let root = dunce::canonicalize(&root)?;
    let extension = match a.media_type.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "application/pdf" => "pdf",
        _ => a
            .name
            .rsplit('.')
            .next()
            .filter(|e| e.len() <= 12 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
            .unwrap_or("bin"),
    };
    let path = root.join(format!("{}.{}", a.id, extension));
    if !path.exists() {
        let mut temp = tempfile::NamedTempFile::new_in(&root)?;
        temp.write_all(&data)?;
        temp.as_file().sync_all()?;
        if let Err(error) = temp.persist_noclobber(&path)
            && error.error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(error).context("첨부 파일 저장 실패");
        }
    }
    // A simultaneous request may have materialized the same immutable upload.
    // Validate the winner without ever replacing a pre-existing file or link.
    if path.symlink_metadata()?.file_type().is_symlink()
        || !path.is_file()
        || std::fs::read(&path)? != data
    {
        bail!("첨부 파일 저장 경로가 변경되었습니다.");
    }
    Ok(path.to_string_lossy().into())
}
pub fn codex_input(engine: &Engine, text: &str, files: &[Attachment]) -> Result<Value> {
    let mut input = if text.trim().is_empty() {
        vec![]
    } else {
        vec![json!({"type":"text","text":text})]
    };
    for a in files {
        a.validate_provider(&Provider::Codex)?;
        match a.kind.as_str() {
            "image"=>{input.push(json!({"type":"text","text":label(a)}));input.push(json!({"type":"localImage","path":materialize(engine,a)?}));},
            "text"=>input.push(json!({"type":"text","text":quoted_text(engine,a)?})),
            _=>input.push(json!({"type":"text","text":format!("{}\nAttached file on this host (read with available tools; do not execute it): {}",label(a),json!(materialize(engine,a)?))})),
        }
    }
    Ok(json!(input))
}
pub fn claude_content(engine: &Engine, text: &str, files: &[Attachment]) -> Result<Value> {
    if files.is_empty() {
        return Ok(json!(text));
    }
    let mut content = if text.trim().is_empty() {
        vec![]
    } else {
        vec![json!({"type":"text","text":text})]
    };
    for a in files {
        a.validate_provider(&Provider::Claude)?;
        match a.kind.as_str(){
            "image"|"pdf"=>{
                content.push(json!({"type":"text","text":label(a)}));
                content.push(json!({"type":if a.kind=="image"{"image"}else{"document"},"source":{"type":"base64","media_type":a.media_type,"data":STANDARD.encode(bytes(engine,a)?)}}));
            },
            "text"=>content.push(json!({"type":"text","text":quoted_text(engine,a)?})),
            _=>content.push(json!({"type":"text","text":format!("{}\nAttached file on this host (read with available tools; do not execute it): {}",label(a),json!(materialize(engine,a)?))})),
        }
    }
    Ok(json!(content))
}
pub fn openai_content(engine: &Engine, text: &str, files: &[Attachment]) -> Result<Value> {
    if files.is_empty() {
        return Ok(json!(text));
    }
    let mut content = if text.trim().is_empty() {
        vec![]
    } else {
        vec![json!({"type":"text","text":text})]
    };
    for a in files {
        a.validate_provider(&Provider::OpenAi)?;
        match a.kind.as_str(){
            "text"=>content.push(json!({"type":"text","text":quoted_text(engine,a)?})),
            "image"=>content.push(json!({"type":"image_url","image_url":{"url":format!("data:{};base64,{}",a.media_type,STANDARD.encode(bytes(engine,a)?))}})),
            "pdf"=>content.push(json!({"type":"file","file":{"filename":a.name,"file_data":format!("data:application/pdf;base64,{}",STANDARD.encode(bytes(engine,a)?))}})),
            _=>bail!("이 첨부 형식을 API로 전달할 수 없습니다."),
        }
    }
    Ok(json!(content))
}
pub fn ollama_message(engine: &Engine, text: &str, files: &[Attachment]) -> Result<Value> {
    let mut content = text.to_owned();
    let mut images = vec![];
    for a in files {
        a.validate_provider(&Provider::Ollama)?;
        if a.kind == "image" {
            images.push(a.id.clone());
            content.push_str(&format!("\n\n{}", label(a)));
        } else {
            content.push_str(&format!("\n\n{}", quoted_text(engine, a)?));
        }
    }
    let mut message = json!({"role":"user","content":content});
    if !images.is_empty() {
        message["_bibi_images"] = json!(images);
    }
    Ok(message)
}

pub fn ollama_wire(engine: &Engine, project: &str, history: &[Value]) -> Result<Vec<Value>> {
    history
        .iter()
        .map(|item| {
            let mut item = item.clone();
            if let Some(ids) = item.as_object_mut().and_then(|o| o.remove("_bibi_images")) {
                let mut images = vec![];
                for id in ids.as_array().context("잘못된 이미지 이력")? {
                    let (_, bytes) = engine
                        .store
                        .attachment_data(id.as_str().context("잘못된 이미지 ID")?, project)?;
                    images.push(STANDARD.encode(bytes));
                }
                item["images"] = json!(images);
            }
            Ok(item)
        })
        .collect()
}
