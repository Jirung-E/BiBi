use super::*;
use serde_json::Value;

fn lease_key(provider: &Provider, native: &str) -> String {
    format!(
        "{}:{native}",
        serde_json::to_string(provider).unwrap_or_default()
    )
}
pub(super) fn ensure_available(c: &Connection, run: &Run) -> Result<()> {
    if run.host_id == "local"
        && let Some(native) = &run.session_key
        && get::<Value>(c, "terminal_lease", &lease_key(&run.provider, native))?
            .is_some_and(|v| v["expires"].as_i64().unwrap_or(0) > now())
    {
        return Err(Error::Conflict(
            "이 세션은 터미널에서 사용 중입니다. CLI를 종료한 뒤 이어가세요.".into(),
        ));
    }
    Ok(())
}
impl Store {
    pub fn catalog_cache(&self, key: &str) -> Result<Option<Value>> {
        self.read(|c| get(c, "catalog_cache", key))
    }
    pub fn save_catalog_cache(&self, key: &str, value: &Value) -> Result<()> {
        self.write(|c| put(c, "catalog_cache", key, "", now(), value))
    }
    pub fn claim_terminal(&self, provider: &Provider, native: &str, ticket: &str) -> Result<()> {
        self.write(|c| {
            let key = lease_key(provider, native);
            if get::<Value>(c, "terminal_lease", &key)?
                .is_some_and(|v| v["expires"].as_i64().unwrap_or(0) > now())
            {
                return Err(Error::Conflict(
                    "같은 세션이 이미 터미널로 열려 있거나 열리는 중입니다.".into(),
                ));
            }
            let runs: Vec<Run> = list(c, "run", None)?;
            for r in runs.iter().filter(|r| {
                r.host_id == "local"
                    && &r.provider == provider
                    && r.session_key.as_deref() == Some(native)
                    && r.origin == Origin::Managed
            }) {
                if r.state.active()
                    || matches!(
                        r.state,
                        RunState::Queued | RunState::Uncertain | RunState::Disconnected
                    )
                    || !message_queue::pending(c, r.session_id())?.is_empty()
                {
                    return Err(Error::Conflict(
                        "BiBi 실행·승인·대기 메시지를 먼저 정리하세요.".into(),
                    ));
                }
            }
            put(
                c,
                "terminal_lease",
                &key,
                "",
                now(),
                &json!({"ticket":ticket,"expires":now()+60_000}),
            )
        })
    }
    pub fn renew_terminal(
        &self,
        provider: &Provider,
        native: &str,
        ticket: &str,
        finish: bool,
    ) -> Result<()> {
        self.write(|c| {
            let key = lease_key(provider, native);
            let mut value: Value = required(c, "terminal_lease", &key)?;
            if value["ticket"] != ticket
                || (!finish && value["expires"].as_i64().unwrap_or(0) <= now())
            {
                return Err(Error::Conflict("터미널 실행 소유권이 만료됐습니다.".into()));
            }
            if finish {
                c.execute(
                    "DELETE FROM entities WHERE kind='terminal_lease' AND id=?1",
                    [&key],
                )?;
            } else {
                value["expires"] = json!(now() + 60_000);
                put(c, "terminal_lease", &key, "", now(), &value)?;
            }
            Ok(())
        })
    }
    pub fn terminal_in_use(&self, provider: &Provider, native: &str) -> Result<bool> {
        self.read(|c| {
            Ok(
                get::<Value>(c, "terminal_lease", &lease_key(provider, native))?
                    .is_some_and(|v| v["expires"].as_i64().unwrap_or(0) > now()),
            )
        })
    }
    pub fn save_terminal_ticket(&self, ticket: &str, value: &Value) -> Result<()> {
        self.write(|c| {
            prune(c)?;
            put(c, "terminal_ticket", ticket, "", now(), value)
        })
    }
    pub fn take_terminal_ticket(&self, ticket: &str) -> Result<Value> {
        self.write(|c| {
            let v: Value = required(c, "terminal_ticket", ticket)?;
            if v["expires"].as_i64().unwrap_or(0) <= now() {
                return Err(Error::Conflict(
                    "터미널 열기 요청이 만료됐습니다. 다시 열어 주세요.".into(),
                ));
            }
            c.execute(
                "DELETE FROM entities WHERE kind='terminal_ticket' AND id=?1",
                [ticket],
            )?;
            Ok(v)
        })
    }
    pub fn queue_terminal_request(&self, id: &str, value: &Value) -> Result<()> {
        self.write(|c| {
            prune(c)?;
            if get::<Value>(c, "terminal_request", id)?.is_some() {
                return Err(Error::Conflict("이미 접수된 터미널 요청입니다.".into()));
            }
            put(
                c,
                "terminal_request",
                id,
                "",
                now(),
                &json!({"id":id,"state":"pending","expires":now()+30_000,"request":value}),
            )
        })
    }
    pub fn terminal_requests(&self) -> Result<Vec<Value>> {
        self.read(|c| {
            Ok(list::<Value>(c, "terminal_request", None)?
                .into_iter()
                .filter(|v| v["state"] == "pending" && v["expires"].as_i64().unwrap_or(0) > now())
                .collect())
        })
    }
    pub fn terminal_request(&self, id: &str) -> Result<Option<Value>> {
        self.read(|c| get(c, "terminal_request", id))
    }
    pub fn update_terminal_request(&self, id: &str, state: &str, result: Value) -> Result<bool> {
        self.write(|c| {
            let mut v: Value = required(c, "terminal_request", id)?;
            if v["state"] == "cancelled" || v["expires"].as_i64().unwrap_or(0) <= now() {
                return Ok(false);
            }
            if state == "processing" && v["state"] != "pending" {
                return Ok(false);
            }
            v["state"] = json!(state);
            v["result"] = result;
            put(c, "terminal_request", id, "", now(), &v)?;
            Ok(true)
        })
    }
}

fn prune(c: &Connection) -> Result<()> {
    c.execute("DELETE FROM entities WHERE kind IN ('terminal_ticket','terminal_request','terminal_lease') AND json_extract(data,'$.expires') < ?1",[now()-60_000])?;
    Ok(())
}
