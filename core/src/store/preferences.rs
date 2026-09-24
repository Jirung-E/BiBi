use super::*;

pub(super) fn remember(c: &Connection, selection: &ModelSelection, used: bool) -> Result<()> {
    let _ = connection(c, &selection.provider_id)?;
    if selection.model.len() > 256 {
        return Err(Error::Invalid("모델 이름은 256자 이하여야 합니다.".into()));
    }
    put(c, "setting", "model_selection", "", now(), selection)?;
    emit(c, "model_selection", selection)?;
    record_model(c, selection, used)
}
fn record_model(c: &Connection, selection: &ModelSelection, used: bool) -> Result<()> {
    if !selection.model.trim().is_empty() {
        let key = format!("{}:{}", selection.provider_id, selection.model);
        let mut item = get::<ModelHistory>(c, "model_history", &key)?.unwrap_or(ModelHistory {
            provider_id: selection.provider_id.clone(),
            model: selection.model.clone(),
            uses: 0,
            last_used: now(),
        });
        item.last_used = now();
        if used {
            item.uses = item.uses.saturating_add(1);
        }
        put(
            c,
            "model_history",
            &key,
            &selection.provider_id,
            now(),
            &item,
        )?;
        emit(c, "model_history", &item)?;
    }
    Ok(())
}

pub(super) fn connection(c: &Connection, id: &str) -> Result<ProviderConfig> {
    if let Some(p) = get(c, "provider", id)? {
        Ok(p)
    } else {
        required(c, "remote_provider", id)
    }
}
pub(super) fn all_connections(c: &Connection) -> Result<Vec<ProviderConfig>> {
    let mut providers = list(c, "provider", None)?;
    providers.extend(list::<ProviderConfig>(c, "remote_provider", None)?);
    Ok(providers)
}
impl Store {
    pub fn replace_remote_providers(
        &self,
        host: &str,
        providers: Vec<ProviderConfig>,
    ) -> Result<()> {
        self.write(|c| {
            let mut ids = Vec::new();
            for mut provider in providers.into_iter().filter(|p| p.host_id == "local") {
                provider.remote_id = Some(provider.id.clone());
                provider.id = remote_provider_id(host, &provider.id);
                provider.host_id = host.into();
                provider.api_key_set = false;
                ids.push(provider.id.clone());
                let old: Option<ProviderConfig> = get(c, "remote_provider", &provider.id)?;
                if old.as_ref().map(serde_json::to_value).transpose()?
                    != Some(serde_json::to_value(&provider)?)
                {
                    put(c, "remote_provider", &provider.id, host, now(), &provider)?;
                    emit(c, "provider", &provider)?;
                }
            }
            for old in list::<ProviderConfig>(c, "remote_provider", Some(host))?
                .into_iter()
                .filter(|p| !ids.contains(&p.id))
            {
                c.execute(
                    "DELETE FROM entities WHERE kind='remote_provider' AND id=?1",
                    [&old.id],
                )?;
                emit(c, "provider_deleted", &json!({"id":old.id}))?;
            }
            Ok(())
        })
    }

    pub fn providers(&self) -> Result<Vec<ProviderConfig>> {
        self.read(all_connections)
    }

    pub fn provider(&self, id: &str) -> Result<ProviderConfig> {
        self.read(|c| connection(c, id))
    }

    pub fn provider_secret(&self, id: &str) -> Result<Option<String>> {
        self.read(|c| get(c, "provider_secret", id))
    }

    pub fn save_provider(
        &self,
        mut provider: ProviderConfig,
        api_key: Option<String>,
    ) -> Result<ProviderConfig> {
        if provider.host_id != "local" || provider.remote_id.is_some() {
            return Err(Error::Invalid(
                "원격 제공자는 해당 호스트에서 편집하세요.".into(),
            ));
        }
        provider.name = provider.name.trim().into();
        provider.command = provider.command.trim().into();
        provider.endpoint = provider.endpoint.trim().trim_end_matches('/').into();
        if provider.id.is_empty()
            || provider.id.len() > 128
            || !provider
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || provider.name.is_empty()
            || provider.name.len() > 160
            || provider.args.len() > 64
            || provider.args.iter().any(|a| a.len() > 8192)
        {
            return Err(Error::Invalid(
                "제공자 ID·이름·실행 인자를 확인하세요.".into(),
            ));
        }
        if matches!(
            provider.adapter,
            Provider::Codex | Provider::Claude | Provider::Command
        ) && provider.command.is_empty()
        {
            return Err(Error::Invalid(
                "실행 명령 또는 실행 파일 경로가 필요합니다.".into(),
            ));
        }
        if matches!(provider.adapter, Provider::Ollama | Provider::OpenAi)
            && !(provider.endpoint.starts_with("http://")
                || provider.endpoint.starts_with("https://"))
        {
            return Err(Error::Invalid(
                "http 또는 https API 주소가 필요합니다.".into(),
            ));
        }
        if provider.adapter == Provider::Ollama {
            if let Some(options) = &provider.ollama {
                options.validate()?;
            }
        } else {
            provider.ollama = None;
        }
        provider.models = provider
            .models
            .into_iter()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        provider.models.sort();
        provider.models.dedup();
        if provider.models.len() > 200 || provider.models.iter().any(|m| m.len() > 256) {
            return Err(Error::Invalid("모델 목록이 너무 큽니다.".into()));
        }
        self.write(|c| {
            if list::<Run>(c, "run", None)?.iter().any(|r| {
                r.provider_id.as_deref() == Some(&provider.id)
                    && matches!(
                        r.state,
                        RunState::Queued
                            | RunState::Running
                            | RunState::WaitingUser
                            | RunState::WaitingExpert
                    )
            }) {
                return Err(Error::Conflict(
                    "이 제공자의 실행이 끝난 뒤 설정을 변경하세요.".into(),
                ));
            }
            if let Some(secret) = api_key {
                if secret.is_empty() {
                    c.execute(
                        "DELETE FROM entities WHERE kind='provider_secret' AND id=?1",
                        [&provider.id],
                    )?;
                } else {
                    put(c, "provider_secret", &provider.id, "", now(), &secret)?;
                }
            }
            provider.api_key_set = get::<String>(c, "provider_secret", &provider.id)?.is_some();
            put(c, "provider", &provider.id, "", now(), &provider)?;
            emit(c, "provider", &provider)?;
            Ok(provider)
        })
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        self.write(|c| {
            let _: ProviderConfig = required(c, "provider", id)?;
            if list::<Run>(c, "run", None)?.iter().any(|r| {
                r.provider_id.as_deref() == Some(id)
                    && matches!(
                        r.state,
                        RunState::Queued
                            | RunState::Running
                            | RunState::WaitingUser
                            | RunState::WaitingExpert
                    )
            }) {
                return Err(Error::Conflict(
                    "이 제공자의 실행이 끝난 뒤 제거하세요.".into(),
                ));
            }
            c.execute(
                "DELETE FROM entities WHERE kind IN ('provider','provider_secret') AND id=?1",
                [id],
            )?;
            c.execute(
                "DELETE FROM entities WHERE kind='quota' AND json_extract(data,'$.provider_id')=?1",
                [id],
            )?;
            if get::<ModelSelection>(c, "setting", "model_selection")?
                .is_some_and(|s| s.provider_id == id)
            {
                c.execute(
                    "DELETE FROM entities WHERE kind='setting' AND id='model_selection'",
                    [],
                )?;
            }
            emit(c, "provider_deleted", &json!({"id":id}))
        })
    }

    pub fn select_model(&self, selection: ModelSelection) -> Result<()> {
        self.write(|c| remember(c, &selection, false))
    }

    pub fn replace_connection_quotas(
        &self,
        provider_id: &str,
        adapter: &Provider,
        mut quotas: Vec<Quota>,
    ) -> Result<()> {
        self.write(|c| {
            for old in list::<Quota>(c,"quota",Some("local"))?.into_iter().filter(|q|q.provider_id.as_deref()==Some(provider_id)) {
                c.execute("DELETE FROM entities WHERE kind='quota' AND id=?1",[old.id])?;
            }
            for quota in &mut quotas {
                quota.provider_id=Some(provider_id.into());
                if quota.id!=format!("local:{provider_id}") && !quota.id.starts_with(&format!("local:{provider_id}:")) { quota.id=format!("local:{provider_id}:{}",quota.id); }
                put(c,"quota",&quota.id,"local",now(),quota)?;
            }
            emit(c,"quotas",&json!({"provider":adapter,"provider_id":provider_id,"host_id":"local","quotas":quotas}))
        })
    }

    pub fn rename_session(&self, run_id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 120 {
            return Err(Error::Invalid("세션 이름은 1~120자여야 합니다.".into()));
        }
        self.write(|c| {
            let run: Run = required(c, "run", run_id)?;
            put(c, "session_title", run.session_id(), "", now(), &title)?;
            for mut item in list::<Run>(c, "run", None)?
                .into_iter()
                .filter(|r| r.session_id() == run.session_id())
            {
                item.title = title.into();
                save_run(c, &item)?;
            }
            Ok(())
        })
    }

    pub fn set_session_hidden(&self, run_id: &str, hidden: bool) -> Result<()> {
        self.write(|c| {
            let run: Run = required(c, "run", run_id)?;
            if hidden
                && list::<Run>(c, "run", None)?.iter().any(|r| {
                    r.session_id() == run.session_id()
                        && r.origin == Origin::Managed
                        && matches!(
                            r.state,
                            RunState::Queued
                                | RunState::Running
                                | RunState::WaitingUser
                                | RunState::WaitingExpert
                        )
                })
            {
                return Err(Error::Conflict(
                    "진행 중인 세션은 중단한 뒤 제거하세요.".into(),
                ));
            }
            if hidden {
                put(
                    c,
                    "hidden_session",
                    run.session_id(),
                    "",
                    now(),
                    &run.session_id(),
                )?;
            } else {
                c.execute(
                    "DELETE FROM entities WHERE kind='hidden_session' AND id=?1",
                    [run.session_id()],
                )?;
            }
            emit(
                c,
                "session_visibility",
                &json!({"session_id":run.session_id(),"hidden":hidden}),
            )
        })
    }

    pub fn runtime_metadata(
        &self,
        run_id: &str,
        model: Option<&str>,
        commands: Option<Vec<SlashCommand>>,
        session_file: Option<String>,
    ) -> Result<()> {
        self.write(|c| {
            let mut run: Run = required(c, "run", run_id)?;
            if let Some(model) = model.filter(|s| !s.trim().is_empty() && s.len() <= 256) {
                if run.agent_kind != "subagent"
                    && run.model.trim().is_empty()
                    && let Some(provider_id) = &run.provider_id
                {
                    let selection = ModelSelection {
                        provider_id: provider_id.clone(),
                        model: model.into(),
                    };
                    if get::<ModelSelection>(c, "setting", "model_selection")?
                        .is_some_and(|s| s.provider_id == *provider_id && s.model.is_empty())
                    {
                        remember(c, &selection, true)?;
                    } else {
                        record_model(c, &selection, true)?;
                    }
                }
                run.model = model.into();
            }
            if let Some(commands) = commands {
                run.runtime.commands = commands;
            }
            if session_file.is_some() {
                run.runtime.session_file = session_file;
            }
            save_run(c, &run)
        })
    }
}
