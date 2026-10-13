use super::*;

pub(super) fn pending(c: &Connection, session: &str) -> Result<Vec<QueuedMessage>> {
    let mut items = list::<QueuedMessage>(c, "queued_message", Some(session))?;
    items.retain(|q| !matches!(q.state.as_str(), "sent" | "cancelled"));
    items.sort_by_key(|q| q.order);
    Ok(items)
}
pub(super) fn receipt(c: &Connection, q: &QueuedMessage) -> Result<Receipt> {
    if let Some(receipt) = &q.receipt {
        return Ok(receipt.clone());
    }
    let run: Run = required(c, "run", q.request.target_run_id.as_deref().unwrap_or(""))?;
    Ok(Receipt {
        submission_id: q.id.clone(),
        run_id: run.id,
        request_id: run.request_id,
        work_id: run.work_id,
        status: q.state.clone(),
    })
}
fn save(c: &Connection, q: &QueuedMessage) -> Result<()> {
    put(c, "queued_message", &q.id, &q.session_id, q.created_at, q)?;
    emit(c, "queued_message", q)
}
fn latest(c: &Connection, session: &str) -> Result<Run> {
    let all = list::<Run>(c, "run", None)?;
    let previous = all
        .iter()
        .filter_map(|r| r.continued_from.as_deref())
        .collect::<std::collections::HashSet<_>>();
    all.iter()
        .filter(|r| r.session_id() == session && !previous.contains(r.id.as_str()))
        .max_by_key(|r| (r.created_at, r.updated_at))
        .cloned()
        .ok_or_else(|| Error::NotFound("대기 메시지의 세션".into()))
}
impl Store {
    pub fn enqueue_message(&self, request: Submission) -> Result<Receipt> {
        Self::validate_submission(&request)?;
        if request.mode != SubmitMode::Continue {
            return Err(Error::Invalid(
                "대기열은 기존 대화 이어가기만 지원합니다.".into(),
            ));
        }
        self.write(|c| {
            if let Some(old) = get::<QueuedMessage>(c, "queued_message", &request.submission_id)? {
                let mut retry = request.clone();
                retry.approval_mode = Some(
                    retry
                        .approval_mode
                        .unwrap_or(old.request.approval_mode.unwrap_or_default()),
                );
                if serde_json::to_value(&old.request)? != serde_json::to_value(&retry)? {
                    return Err(Error::Conflict(
                        "같은 전송 ID에 다른 대기 메시지가 있습니다.".into(),
                    ));
                }
                return receipt(c, &old);
            }
            if c.query_row(
                "SELECT EXISTS(SELECT 1 FROM submissions WHERE id=?1)",
                [&request.submission_id],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(Error::Conflict("이미 사용한 전송 ID입니다.".into()));
            }
            let run: Run = required(c, "run", request.target_run_id.as_deref().unwrap_or(""))?;
            super::terminal::ensure_available(c, &run)?;
            if run.origin != Origin::Managed
                || !run.capabilities.continue_session.supported
                || run.agent_kind == "subagent"
                || get::<String>(c, "hidden_session", run.session_id())?.is_some()
            {
                return Err(Error::Conflict(
                    "이 세션에는 메시지를 대기시킬 수 없습니다.".into(),
                ));
            }
            if run.project_key != request.project_key
                || run.provider != request.provider
                || run.provider_id != request.provider_id
                || run.host_id != request.host_id
                || run.role != request.role
                || run.read_only != request.read_only
                || request.work_id.as_ref().is_some_and(|w| w != &run.work_id)
            {
                return Err(Error::Conflict(
                    "대기 메시지의 실행 대상이 다릅니다.".into(),
                ));
            }
            if get::<String>(
                c,
                "setting",
                &format!("native_release:{}", run.session_id()),
            )?
            .as_deref()
                == Some("releasing")
            {
                return Err(Error::Conflict(
                    "터미널 전환 중에는 메시지를 대기시킬 수 없습니다.".into(),
                ));
            }
            request
                .approval_mode
                .unwrap_or(run.approval_mode)
                .validate(&run.provider, run.read_only)?;
            attachments::validate(c, &request)?;
            if pending(c, run.session_id())?.len() >= 50 {
                return Err(Error::Invalid(
                    "한 대화에는 최대 50개 메시지를 대기시킬 수 있습니다.".into(),
                ));
            }
            let order = list::<QueuedMessage>(c, "queued_message", Some(run.session_id()))?
                .iter()
                .map(|q| q.order)
                .max()
                .unwrap_or(0)
                + 1;
            let mut request = request;
            request.approval_mode = Some(request.approval_mode.unwrap_or(run.approval_mode));
            let q = QueuedMessage {
                id: request.submission_id.clone(),
                session_id: run.session_id().into(),
                order,
                request,
                state: "queued".into(),
                created_at: now(),
                error: None,
                dispatch: None,
                receipt: None,
            };
            save(c, &q)?;
            receipt(c, &q)
        })
    }
    pub fn change_queued_message(&self, id: &str, cancel: bool) -> Result<QueuedMessage> {
        self.write(|c| {
            let mut q: QueuedMessage = required(c, "queued_message", id)?;
            if !matches!(q.state.as_str(), "queued" | "blocked" | "ready") {
                return Err(Error::Conflict(
                    "이미 전달을 준비한 메시지는 해당 실행에서 중단하세요.".into(),
                ));
            }
            q.state = if cancel { "cancelled" } else { "ready" }.into();
            q.error = None;
            q.dispatch = None;
            save(c, &q)?;
            Ok(q)
        })
    }
    pub fn queued_message_ids(&self) -> Result<Vec<String>> {
        self.read(|c| {
            Ok(list::<QueuedMessage>(c, "queued_message", None)?
                .into_iter()
                .filter(|q| matches!(q.state.as_str(), "queued" | "ready" | "dispatching"))
                .map(|q| q.id)
                .collect())
        })
    }
    pub fn prepare_queued_message(&self, id: &str) -> Result<Option<Submission>> {
        self.write(|c|{
            let mut q:QueuedMessage=required(c,"queued_message",id)?;
            if pending(c,&q.session_id)?.first().is_none_or(|first|first.id!=q.id){return Ok(None);}
            if q.state=="dispatching" {return Ok(q.dispatch);}
            if !matches!(q.state.as_str(),"queued"|"ready"){return Ok(None);}
            let run=latest(c,&q.session_id)?;
            if (run.state.active()&&run.state!=RunState::Disconnected)||run.state==RunState::Queued{return Ok(None);}
            if run.state==RunState::Uncertain||run.state==RunState::Disconnected||(run.state!=RunState::Completed&&q.state!="ready") {
                q.state="blocked".into();q.error=Some("이전 실행이 중단·실패했거나 연결 확인이 필요합니다. 확인 후 대기열을 재개하세요.".into());save(c,&q)?;return Ok(None);
            }
            let mut request=q.request.clone();request.target_run_id=Some(run.id.clone());request.expected_turn_id=run.turn_id.clone();
            q.dispatch=Some(request.clone());q.state="dispatching".into();q.error=None;save(c,&q)?;Ok(Some(request))
        })
    }
    pub fn finish_queued_message(
        &self,
        id: &str,
        result: std::result::Result<Receipt, String>,
    ) -> Result<()> {
        self.write(|c| {
            let mut q: QueuedMessage = required(c, "queued_message", id)?;
            if q.state != "dispatching" {
                return Err(Error::Conflict("대기 메시지 상태가 변경되었습니다.".into()));
            }
            match result {
                Ok(receipt) => {
                    q.receipt = Some(receipt);
                    q.state = "sent".into();
                    q.error = None;
                }
                Err(error) => {
                    q.state = "blocked".into();
                    q.error = Some(error);
                }
            }
            save(c, &q)
        })
    }
}
