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

fn save(c: &Connection, meta: Attachment, data: &[u8]) -> Result<Attachment> {
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
}
impl Store {
    pub fn save_attachment(&self, meta: Attachment, data: &[u8]) -> Result<Attachment> {
        self.write(|c| save(c, meta, data))
    }
    /// Snapshot an explicitly published result and its message in one transaction.
    pub fn publish_attachment(
        &self,
        run_id: &str,
        message_id: Option<&str>,
        meta: Attachment,
        data: &[u8],
    ) -> Result<Attachment> {
        self.write(|c| {
            let run: Run = required(c, "run", run_id)?;
            if run.origin != Origin::Managed
                || run.host_id != "local"
                || run.state.terminal()
                || meta.project_key != run.project_key
            {
                return Err(Error::Conflict(
                    "현재 실행의 작업 폴더에서만 결과를 등록할 수 있습니다.".into(),
                ));
            }
            let messages = list::<Message>(c, "message", Some(run_id))?;
            let files: std::collections::HashMap<_, _> = messages
                .iter()
                .filter(|m| m.role == "assistant")
                .flat_map(|m| &m.attachments)
                .map(|a| (&a.id, a))
                .collect();
            if !files.contains_key(&meta.id)
                && (files.len() >= 8
                    || files.values().map(|a| a.size).sum::<u64>() + meta.size > 16 * 1024 * 1024)
            {
                return Err(Error::Invalid(
                    "한 응답의 결과는 최대 8개 · 합계 16 MiB까지 보관합니다.".into(),
                ));
            }
            let key = message_id
                .map(String::from)
                .unwrap_or_else(|| format!("{run_id}:artifacts"));
            let mut message = if let Some(m) = get::<Message>(c, "message", &key)? {
                m
            } else if message_id.is_none() {
                Message {
                    id: key,
                    run_id: run_id.into(),
                    role: "assistant".into(),
                    text: String::new(),
                    attachments: vec![],
                    phase: Some("artifacts".into()),
                    created_at: now(),
                }
            } else {
                return Err(Error::NotFound("결과 메시지가 없습니다.".into()));
            };
            if message.run_id != run_id || message.role != "assistant" {
                return Err(Error::Conflict("다른 실행의 메시지입니다.".into()));
            }
            let meta = save(c, meta, data)?;
            if !message.attachments.iter().any(|a| a.id == meta.id) {
                message.attachments.push(meta.clone());
                put(
                    c,
                    "message",
                    &message.id,
                    run_id,
                    message.created_at,
                    &message,
                )?;
                emit(c, "message", &message)?;
            }
            Ok(meta)
        })
    }
    pub fn finish_artifacts(&self, run_id: &str) -> Result<()> {
        self.write(|c| {
            let key = format!("{run_id}:artifacts");
            let Some(pending) = get::<Message>(c, "message", &key)? else {
                return Ok(());
            };
            let Some(mut last) = list::<Message>(c, "message", Some(run_id))?
                .into_iter()
                .rev()
                .find(|m| {
                    m.role == "assistant"
                        && !m.text.is_empty()
                        && m.phase.as_deref() != Some("commentary")
                })
            else {
                return Ok(());
            };
            for a in pending.attachments {
                if !last.attachments.iter().any(|old| old.id == a.id) {
                    last.attachments.push(a);
                }
            }
            put(c, "message", &last.id, run_id, last.created_at, &last)?;
            c.execute("DELETE FROM entities WHERE kind='message' AND id=?1", [key])?;
            emit(c, "message", &last)
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
