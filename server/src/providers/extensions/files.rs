use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
use toml_edit::{DocumentMut, Item};

pub const MASK: &str = "__BIBI_KEEP_SECRET__";
pub const LIMIT: u64 = 2 * 1024 * 1024;
pub fn text(path: &Path) -> Result<String> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > LIMIT => bail!("설정 파일이 너무 큽니다: {}", path.display()),
        Ok(_) => fs::read_to_string(path)
            .with_context(|| format!("설정 파일을 읽지 못했습니다: {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => bail!("설정 파일에 접근할 수 없습니다: {}", path.display()),
    }
}
pub fn revision(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
pub fn json_file(path: &Path) -> Result<Value> {
    let raw = text(path)?;
    if raw.is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(&raw)
        .with_context(|| format!("JSON 설정 형식 오류: {}", path.display()))?;
    if !value.is_object() {
        bail!("설정은 JSON 객체여야 합니다: {}", path.display());
    }
    Ok(value)
}
pub fn toml_file(path: &Path) -> Result<(DocumentMut, Value)> {
    let raw = text(path)?;
    let doc = raw
        .parse::<DocumentMut>()
        .with_context(|| format!("TOML 설정 형식 오류: {}", path.display()))?;
    let value =
        toml_edit::de::from_str::<Value>(&raw).context("TOML 설정 구조를 읽지 못했습니다.")?;
    Ok((doc, value))
}
pub fn item(value: &Value) -> Result<Item> {
    let raw = toml_edit::ser::to_string(&json!({"value":value}))
        .context("TOML로 저장할 수 없는 값입니다.")?;
    Ok(raw.parse::<DocumentMut>()?["value"].clone())
}
/// Refuse traversal and symlink components, even when the final file does not exist.
pub fn scoped(root: &Path, relative: &Path) -> Result<PathBuf> {
    let root = root.canonicalize().context("프로젝트 폴더가 없습니다.")?;
    let mut path = root;
    for component in relative.components() {
        let Component::Normal(part) = component else {
            bail!("프로젝트 내부 경로만 사용할 수 있습니다.");
        };
        path.push(part);
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("심볼릭 링크 설정은 원본 위치에서 관리하세요.");
        }
    }
    Ok(path)
}
pub fn write(path: &Path, expected: &str, value: &str) -> Result<()> {
    if value.len() as u64 > LIMIT {
        bail!("설정이 너무 큽니다.");
    }
    if revision(&text(path)?) != expected {
        bail!("다른 곳에서 설정이 변경되었습니다. 새로고침 후 다시 저장하세요.");
    }
    let parent = path.parent().context("설정 경로 오류")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(value.as_bytes())?;
    file.as_file().sync_all()?;
    // Preserve restrictive permissions, including an existing native configuration.
    if let Ok(meta) = fs::metadata(path) {
        file.as_file().set_permissions(meta.permissions())?;
    }
    if revision(&text(path)?) != expected {
        bail!("저장 중 설정이 변경되었습니다. 새로고침하세요.");
    }
    file.persist(path)
        .map_err(|_| anyhow::anyhow!("설정 저장 실패. 기존 설정을 유지했습니다."))?;
    Ok(())
}
pub fn write_json(path: &Path, expected: &str, value: &Value) -> Result<()> {
    write(
        path,
        expected,
        &(serde_json::to_string_pretty(value)? + "\n"),
    )
}
pub fn safe_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 160
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || "_-:@.".contains(c))
        || name == "."
        || name == ".."
        || name.starts_with('-')
    {
        bail!("이름에는 글자·숫자·밑줄·하이픈·콜론·@만 사용할 수 있습니다.");
    }
    Ok(())
}
// Treat all env/header values and URL credentials as secrets. Redacted fields round-trip
// against the original value on the host; snapshots never contain these configurations.
pub fn redact(value: &Value) -> Value {
    fn walk(value: &Value, key: &str, secret: bool) -> Value {
        let secret = secret
            || matches!(key, "env" | "headers" | "http_headers")
            || [
                "token",
                "secret",
                "password",
                "api_key",
                "apikey",
                "authorization",
            ]
            .iter()
            .any(|s| key.to_lowercase().contains(s));
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), walk(v, k, secret)))
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.iter().map(|v| walk(v, key, secret)).collect()),
            Value::String(s) if secret && !s.is_empty() => json!(MASK),
            Value::String(s)
                if matches!(key, "url" | "endpoint")
                    && reqwest::Url::parse(s).is_ok_and(|u| {
                        !u.username().is_empty() || u.password().is_some() || u.query().is_some()
                    }) =>
            {
                json!(MASK)
            }
            _ => value.clone(),
        }
    }
    walk(value, "", false)
}
pub fn restore(value: &mut Value, old: &Value) -> Result<()> {
    if value.as_str() == Some(MASK) {
        if old.is_null() {
            bail!("보존할 인증 정보가 없습니다. 값을 입력하세요.");
        }
        *value = old.clone();
    } else if let Some(map) = value.as_object_mut() {
        for (k, v) in map {
            restore(v, &old[k])?;
        }
    } else if let Some(a) = value.as_array_mut() {
        for (i, v) in a.iter_mut().enumerate() {
            restore(v, &old[i])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_placeholders_preserve_host_values_and_stale_writes_do_not_clobber_edits() {
        let original = json!({"command":"server","env":{"TOKEN":"never-echo"},"headers":{"Authorization":"secret"},"url":"https://server/mcp?token=hidden"});
        let mut draft = redact(&original);
        assert!(!draft.to_string().contains("never-echo"));
        assert!(!draft.to_string().contains("hidden"));
        draft["command"] = json!("new-command");
        restore(&mut draft, &original).unwrap();
        assert_eq!(draft["env"], original["env"]);
        assert_eq!(draft["url"], original["url"]);
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("settings.json");
        write_json(&file, &revision(""), &original).unwrap();
        let old = text(&file).unwrap();
        std::fs::write(&file, "external edit").unwrap();
        assert!(write_json(&file, &revision(&old), &draft).is_err());
        assert_eq!(text(&file).unwrap(), "external edit");
    }
}
