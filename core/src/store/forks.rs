use super::*;

impl Store {
    pub fn fork_request(&self, request: &ForkRequest) -> Result<Option<Run>> {
        self.read(|c| {
            let key = format!("session_fork_request:{}", request.request_id);
            let Some(old) = get::<serde_json::Value>(c, "setting", &key)? else { return Ok(None); };
            if old["request"] != serde_json::to_value(request)? {
                return Err(Error::Conflict("같은 포크 요청 ID의 내용이 변경되었습니다.".into()));
            }
            match old["run_id"].as_str() {
                Some(id) => required(c, "run", id).map(Some),
                None => Err(Error::Conflict("이 포크 요청의 완료를 확인하지 못했습니다. 원본은 보존되어 있습니다. 세션 목록을 확인한 뒤 다시 시도하세요.".into())),
            }
        })
    }
    pub fn begin_fork(&self, request: &ForkRequest) -> Result<()> {
        if request.request_id.is_empty()
            || request.request_id.len() > 128
            || request.title.trim().is_empty()
            || request.title.chars().count() > 120
        {
            return Err(Error::Invalid(
                "포크 요청 ID와 120자 이하의 이름이 필요합니다.".into(),
            ));
        }
        self.write(|c| {
            let key = format!("session_fork_request:{}", request.request_id);
            if get::<serde_json::Value>(c, "setting", &key)?.is_some() {
                return Err(Error::Conflict("이미 접수된 포크 요청입니다.".into()));
            }
            put(c, "setting", &key, "", now(), &json!({"request":request}))
        })
    }
    // The native fork is prepared first. Publish history, membership and lineage
    // together; a fork is idle and never enters the model dispatch queue.
    pub fn finish_fork(
        &self,
        request: &ForkRequest,
        source: &Run,
        content: ForkContent,
    ) -> Result<Run> {
        let ForkContent {
            native,
            session_file,
            messages,
            history,
            claude_root,
        } = content;
        self.write(|c| {
            let key = format!("session_fork_request:{}", request.request_id);
            let record: serde_json::Value = required(c, "setting", &key)?;
            if record["request"] != serde_json::to_value(request)? || !record["run_id"].is_null() {
                return Err(Error::Conflict("포크 요청 상태가 변경되었습니다.".into()));
            }
            let current: Run = required(c, "run", &source.id)?;
            if request.run_id != source.id
                || current.session_key != source.session_key
                || current.provider_id != source.provider_id
                || current.workspace != source.workspace
                || current.host_id != source.host_id
            {
                return Err(Error::Conflict(
                    "원본 연결이 변경되었습니다. 다시 확인하세요.".into(),
                ));
            }
            let mut run = source.clone();
            run.id = id("run");
            run.session_id = id("session");
            run.request_id = id("request");
            run.continued_from = None;
            run.parent_run_id = None;
            run.parent_session_id = None;
            run.agent_kind = "session".into();
            run.title = request.title.trim().into();
            run.session_key = Some(native);
            run.turn_id = None;
            run.origin = Origin::Managed;
            run.capabilities = Capabilities::managed(&run.provider);
            run.state = RunState::Completed;
            run.phase = "포크 준비됨".into();
            run.wait_reason = None;
            run.activity = None;
            run.error = None;
            run.stats = UsageStats::default();
            run.created_at = now();
            run.updated_at = run.created_at;
            run.observed_at = run.created_at;
            run.observation_source = "session/fork".into();
            run.runtime.session_file = session_file;
            run.runtime.fork = Some(ForkOrigin {
                session_id: source.session_id().into(),
                run_id: source.id.clone(),
                point_id: request.point_id.clone(),
            });
            run.context.question.clear();
            run.context.request_id = run.request_id.clone();
            run.context.parent_request_id = None;
            run.context.reply_to_response_id = None;
            run.context.previous_answer_excerpt = None;
            run.context.source_runs.clear();
            run.context.references.clear();
            run.context.reply_to =
                format!("bibi://inbox/{}/{}", run.conversation_id, run.request_id);
            save_run(c, &run)?;
            for mut message in messages {
                message.id = id("message");
                message.run_id = run.id.clone();
                put(
                    c,
                    "message",
                    &message.id,
                    &run.id,
                    message.created_at,
                    &message,
                )?;
            }
            if let Some(history) = history {
                put(
                    c,
                    "setting",
                    &format!("ollama:history:{}", run.id),
                    "",
                    now(),
                    &history,
                )?;
            }
            if let Some(root) = claude_root {
                put(
                    c,
                    "setting",
                    &format!("claude_root:{}", run.session_id()),
                    "",
                    now(),
                    &root,
                )?;
            }
            let mut groups = get::<SessionGroups>(c, "session_groups", source.session_id())?
                .unwrap_or(SessionGroups {
                    session_id: source.session_id().into(),
                    work_ids: vec![source.work_id.clone()],
                });
            groups.session_id = run.session_id().into();
            put(
                c,
                "session_groups",
                run.session_id(),
                &run.project_key,
                now(),
                &groups,
            )?;
            emit(c, "session_groups", &groups)?;
            put(
                c,
                "setting",
                &key,
                "",
                now(),
                &json!({"request":request,"run_id":run.id}),
            )?;
            Ok(run)
        })
    }
}
