use super::*;

pub(crate) fn metadata(c: &Connection, id: &str, project: &str) -> Result<Attachment> {
    let data: Option<String> = c
        .query_row(
            "SELECT metadata FROM attachments WHERE id=?1 AND project_key=?2",
            params![id, project],
            |r| r.get(0),
        )
        .optional()?;
    serde_json::from_str(&data.ok_or_else(|| {
        Error::NotFound("첨부 파일을 찾을 수 없습니다. 파일을 다시 선택하세요.".into())
    })?)
    .map_err(Error::from)
}

pub(crate) fn validate(c: &Connection, request: &Submission) -> Result<Vec<Attachment>> {
    if request.attachments.is_empty() {
        return Ok(vec![]);
    }
    if request.attachments.len() > 8 {
        return Err(Error::Invalid(
            "한 메시지에 파일을 8개까지 첨부할 수 있습니다.".into(),
        ));
    }
    if request.host_id != "local" {
        return Err(Error::Unsupported("이 호스트의 BiBi에 직접 접속해서 첨부하세요. 다른 호스트로의 첨부 전달은 아직 지원하지 않습니다.".into()));
    }
    if crate::slash::parse(&request.question).is_some() {
        return Err(Error::Unsupported(
            "슬래시 명령과 첨부 파일은 따로 보내세요.".into(),
        ));
    }
    if request.mode == SubmitMode::Steer && request.provider != Provider::Codex {
        return Err(Error::Unsupported(
            "이 제공자의 첨부는 현재 응답이 끝난 뒤 보낼 수 있습니다.".into(),
        ));
    }
    let mut ids = std::collections::HashSet::new();
    let files = request
        .attachments
        .iter()
        .map(|id| {
            if !ids.insert(id) {
                return Err(Error::Invalid("같은 첨부가 중복되었습니다.".into()));
            }
            let a = metadata(c, id, &request.project_key)?;
            a.validate_provider(&request.provider)?;
            if request.read_only && request.provider == Provider::Claude && a.kind == "file" {
                return Err(Error::Unsupported(
                    "읽기 전용 Claude 세션에서는 이미지·텍스트·PDF를 첨부하세요.".into(),
                ));
            }
            Ok(a)
        })
        .collect::<Result<Vec<_>>>()?;
    if files.iter().map(|a| a.size).sum::<u64>() > 16 * 1024 * 1024 {
        return Err(Error::Invalid(
            "첨부 파일의 합계는 16 MiB 이하여야 합니다.".into(),
        ));
    }
    Ok(files)
}

impl Store {
    pub fn save_attachment(&self, meta: Attachment, data: &[u8]) -> Result<Attachment> {
        if meta.id.len() != 36
            || !meta.id.starts_with("att_")
            || !meta.id[4..].bytes().all(|c| c.is_ascii_hexdigit())
            || meta.size != data.len() as u64
            || data.len() > 8 * 1024 * 1024
            || meta.sha256 != format!("{:x}", Sha256::digest(data))
        {
            return Err(Error::Invalid(
                "올바른 첨부 ID와 8 MiB 이하의 파일이 필요합니다.".into(),
            ));
        }
        self.write(|c| {
            let _: Project = required(c, "project", &meta.project_key)?;
            let old: Option<String> = c
                .query_row(
                    "SELECT metadata FROM attachments WHERE id=?1",
                    [&meta.id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = old {
                let old: Attachment = serde_json::from_str(&old)?;
                if old != meta {
                    return Err(Error::Conflict(
                        "같은 첨부 ID의 내용이 변경되었습니다.".into(),
                    ));
                }
                return Ok(old);
            }
            c.execute(
                "INSERT INTO attachments(id,project_key,metadata,data) VALUES(?1,?2,?3,?4)",
                params![
                    meta.id,
                    meta.project_key,
                    serde_json::to_string(&meta)?,
                    data
                ],
            )?;
            Ok(meta)
        })
    }
    pub fn attachment(&self, id: &str, project: &str) -> Result<Attachment> {
        self.read(|c| metadata(c, id, project))
    }
    pub fn attachment_data(&self, id: &str, project: &str) -> Result<(Attachment, Vec<u8>)> {
        self.read(|c| {
            let meta = metadata(c, id, project)?;
            let data = c.query_row(
                "SELECT data FROM attachments WHERE id=?1 AND project_key=?2",
                params![id, project],
                |r| r.get(0),
            )?;
            Ok((meta, data))
        })
    }
}
