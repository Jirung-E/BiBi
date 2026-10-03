use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub fn validate(config: &Value) -> Result<()> {
    if !config.is_object() {
        bail!("MCP 설정은 JSON 객체여야 합니다.");
    }
    let command = config["command"].as_str().filter(|s| !s.trim().is_empty());
    let url = config["url"].as_str().filter(|s| !s.trim().is_empty());
    if command.is_some() == url.is_some() {
        bail!("MCP 실행 명령 또는 URL 중 하나를 입력하세요.");
    }
    if let Some(url) = url {
        let parsed = reqwest::Url::parse(url).context("MCP URL 형식 오류")?;
        if !matches!(parsed.scheme(), "http" | "https") {
            bail!("MCP URL은 HTTP 또는 HTTPS여야 합니다.");
        }
    }
    if let Some(args) = config.get("args")
        && (!args.is_array() || args.as_array().unwrap().iter().any(|v| !v.is_string()))
    {
        bail!("MCP 인자는 문자열 배열이어야 합니다.");
    }
    for field in ["env", "headers", "http_headers"] {
        if let Some(map) = config.get(field)
            && (!map.is_object() || map.as_object().unwrap().values().any(|v| !v.is_string()))
        {
            bail!("MCP {field}는 문자열 값의 객체여야 합니다.");
        }
    }
    Ok(())
}
fn expand(value: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        let end = rest.find('}').context("환경 변수 형식 오류")?;
        let expression = &rest[..end];
        let (name, fallback) = expression
            .split_once(":-")
            .map(|(a, b)| (a, Some(b)))
            .unwrap_or((expression, None));
        let resolved = std::env::var(name)
            .ok()
            .or_else(|| fallback.map(String::from))
            .context("MCP에 필요한 호스트 환경 변수가 없습니다.")?;
        out.push_str(&resolved);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}
fn message(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn init() -> Value {
    message(
        1,
        "initialize",
        json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"bibi-check","version":bibi_core::VERSION}}),
    )
}
fn tools(value: &Value) -> Result<(Vec<Value>, Option<String>)> {
    if value.get("error").is_some() {
        bail!("MCP가 도구 목록 요청을 거절했습니다.");
    }
    let items = value["result"]["tools"]
        .as_array()
        .context("MCP tools/list 응답 형식 오류")?;
    if items.len() > 1000 {
        bail!("MCP 도구 목록이 너무 큽니다.");
    }
    let mut out = vec![];
    for item in items {
        let name = item["name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .context("MCP 도구 이름 형식 오류")?;
        out.push(json!({"name":name,"description":item["description"].as_str().map(|s|s.chars().take(1000).collect::<String>())}));
    }
    Ok((
        out,
        value["result"]["nextCursor"].as_str().map(String::from),
    ))
}
async fn line<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R, id: u64) -> Result<Value> {
    for _ in 0..100 {
        let mut raw = Vec::new();
        reader
            .take(super::files::LIMIT + 1)
            .read_until(b'\n', &mut raw)
            .await?;
        if raw.is_empty() {
            bail!("MCP 연결이 종료되었습니다.");
        }
        if raw.len() as u64 > super::files::LIMIT {
            bail!("MCP 응답 크기 초과");
        }
        let value: Value = serde_json::from_slice(&raw).context("MCP JSON 응답 형식 오류")?;
        if value["id"] == id && value.get("method").is_none() {
            return Ok(value);
        }
        // Probes cannot satisfy sampling, elicitation, or tool invocations.
        if value.get("id").is_some() && value.get("method").is_some() {
            bail!("연결 확인 중 MCP가 추가 사용자/모델 작업을 요청했습니다.");
        }
    }
    bail!("MCP 알림 수 초과")
}
async fn send<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}
async fn stdio(config: &Value, root: &Path) -> Result<Value> {
    let program = expand(config["command"].as_str().unwrap())?;
    let mut command = super::super::launch::command(&program)?;
    for arg in config["args"].as_array().into_iter().flatten() {
        command.arg(expand(arg.as_str().unwrap())?);
    }
    for (k, v) in config["env"].as_object().into_iter().flatten() {
        command.env(k, expand(v.as_str().unwrap())?);
    }
    command
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = super::super::launch::spawn(&mut command)?;
    let mut writer = child.stdin.take().context("MCP 입력 없음")?;
    let mut reader = BufReader::new(child.stdout.take().context("MCP 출력 없음")?);
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        send(&mut writer, &init()).await?;
        let response = line(&mut reader, 1).await?;
        if response.get("error").is_some() || !response["result"]["protocolVersion"].is_string() {
            bail!("MCP 초기화 실패");
        }
        send(
            &mut writer,
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await?;
        let mut all = vec![];
        let mut cursor = None;
        for id in 2..22 {
            send(
                &mut writer,
                &message(
                    id,
                    "tools/list",
                    cursor
                        .as_ref()
                        .map(|c| json!({"cursor":c}))
                        .unwrap_or(json!({})),
                ),
            )
            .await?;
            let (items, next) = tools(&line(&mut reader, id).await?)?;
            all.extend(items);
            if all.len() > 1000 {
                bail!("MCP 도구 목록이 너무 큽니다.");
            }
            if next.is_none() {
                return Ok(json!({"connected":true,"tools":all,"model_calls":0}));
            }
            if next == cursor {
                bail!("MCP 페이지 커서 반복 오류");
            }
            cursor = next;
        }
        bail!("MCP 페이지 수 초과")
    })
    .await;
    drop(writer);
    let _ = child.kill().await;
    let _ = child.wait().await;
    result.context("MCP 연결 확인 시간 초과")?
}
async fn http(config: &Value) -> Result<Value> {
    if config["type"] == "sse" {
        bail!("구형 SSE 연결 확인은 지원하지 않습니다. 제공자의 /mcp 화면에서 확인하세요.");
    }
    let url = expand(config["url"].as_str().unwrap())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .build()?;
    let mut headers = reqwest::header::HeaderMap::new();
    for key in ["headers", "http_headers"] {
        for (k, v) in config[key].as_object().into_iter().flatten() {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes())?,
                expand(v.as_str().unwrap())?.parse()?,
            );
        }
    }
    if let Some(name) = config["bearer_token_env_var"].as_str() {
        let token = std::env::var(name).context("MCP 인증 환경 변수가 없습니다.")?;
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {token}").parse()?,
        );
    }
    headers.insert(
        reqwest::header::ACCEPT,
        "application/json, text/event-stream".parse()?,
    );
    let request = |body: Value, headers: reqwest::header::HeaderMap| {
        client.post(&url).headers(headers).json(&body).send()
    };
    let mut first = request(init(), headers.clone())
        .await
        .context("MCP HTTP 연결 실패")?;
    if !first.status().is_success() {
        bail!(
            "MCP HTTP {} · 인증 또는 서버 설정을 확인하세요.",
            first.status().as_u16()
        );
    }
    if let Some(session) = first.headers().get("mcp-session-id") {
        headers.insert("mcp-session-id", session.clone());
    }
    // Keep cleanup outside the deadline, including malformed initialize replies.
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let result = response(&mut first, 1).await?;
        let version = result["result"]["protocolVersion"]
            .as_str()
            .context("MCP 초기화 실패")?;
        headers.insert("mcp-protocol-version", version.parse()?);
        let initialized = request(
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            headers.clone(),
        )
        .await
        .context("MCP 초기화 알림 실패")?;
        if !initialized.status().is_success() {
            bail!("MCP 초기화 알림 거절");
        }
        let mut all = vec![];
        let mut cursor = None;
        for id in 2..22 {
            let mut r = request(
                message(
                    id,
                    "tools/list",
                    cursor
                        .as_ref()
                        .map(|c| json!({"cursor":c}))
                        .unwrap_or(json!({})),
                ),
                headers.clone(),
            )
            .await
            .context("MCP 도구 목록 연결 실패")?;
            if !r.status().is_success() {
                bail!("MCP HTTP {} · 도구 목록 조회 실패", r.status().as_u16());
            }
            let (items, next) = tools(&response(&mut r, id).await?)?;
            all.extend(items);
            if all.len() > 1000 {
                bail!("MCP 도구 목록이 너무 큽니다.");
            }
            if next.is_none() {
                return Ok(json!({"connected":true,"tools":all,"model_calls":0}));
            }
            if next == cursor {
                bail!("MCP 페이지 커서 반복 오류");
            }
            cursor = next;
        }
        bail!("MCP 페이지 수 초과")
    })
    .await
    .context("MCP 연결 확인 시간 초과")
    .and_then(|result| result);
    if headers.contains_key("mcp-session-id") {
        let _ = client
            .delete(&url)
            .headers(headers)
            .timeout(Duration::from_secs(2))
            .send()
            .await;
    }
    result
}
async fn response(r: &mut reqwest::Response, id: u64) -> Result<Value> {
    let sse = r
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.contains("text/event-stream"));
    let mut bytes = vec![];
    while let Some(chunk) = r.chunk().await.context("MCP HTTP 응답 오류")? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > super::files::LIMIT {
            bail!("MCP 응답 크기 초과");
        }
        if sse {
            let text = String::from_utf8_lossy(&bytes);
            for line in text.lines().filter_map(|l| l.strip_prefix("data:")) {
                if let Ok(value) = serde_json::from_str::<Value>(line.trim())
                    && value["id"] == id
                {
                    return Ok(value);
                }
            }
        }
    }
    let value: Value = serde_json::from_slice(&bytes).context("MCP HTTP JSON 응답 오류")?;
    if value["id"] != id {
        bail!("MCP 응답 ID 불일치");
    }
    Ok(value)
}
pub async fn check(config: &Value, root: &Path) -> Result<Value> {
    validate(config)?;
    if config["command"].is_string() {
        stdio(config, root).await
    } else {
        http(config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, http::HeaderMap, routing::post};
    use std::sync::{Arc, Mutex};
    #[tokio::test]
    async fn streamable_http_probe_carries_auth_lists_tools_and_closes_only_its_session() {
        for valid in [true, false] {
            let seen = Arc::new(Mutex::new(vec![]));
            let capture = seen.clone();
            let app=Router::new().route("/mcp",post(move |headers:HeaderMap,Json(body):Json<Value>|{let capture=capture.clone();async move{
    assert_eq!(headers.get("authorization").unwrap(),"Bearer fixture-secret");capture.lock().unwrap().push(body.clone());
    let result=if body["method"]=="initialize"{if valid {json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"test","version":"1"}})} else {json!({})}}else{assert_eq!(headers.get("mcp-session-id").unwrap(),"probe-session");json!({"tools":[{"name":"read-only-list"}]})};
    ([("mcp-session-id","probe-session")],Json(json!({"jsonrpc":"2.0","id":body["id"],"result":result})))
  }}).delete({let seen=seen.clone();move ||{let seen=seen.clone();async move{seen.lock().unwrap().push(json!({"method":"delete"}));axum::http::StatusCode::NO_CONTENT}}}));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/mcp", listener.local_addr().unwrap());
            let server = tokio::spawn(axum::serve(listener, app).into_future());
            let result = check(
                &json!({"url":url,"headers":{"Authorization":"Bearer fixture-secret"}}),
                Path::new("."),
            )
            .await;
            if !valid {
                assert!(result.is_err());
                assert_eq!(
                    seen.lock()
                        .unwrap()
                        .iter()
                        .map(|v| v["method"].as_str().unwrap())
                        .collect::<Vec<_>>(),
                    ["initialize", "delete"]
                );
                server.abort();
                continue;
            }
            let result = result.unwrap();
            assert_eq!(result["tools"][0]["name"], "read-only-list");
            assert!(!result.to_string().contains("fixture-secret"));
            assert_eq!(
                seen.lock()
                    .unwrap()
                    .iter()
                    .map(|v| v["method"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                [
                    "initialize",
                    "notifications/initialized",
                    "tools/list",
                    "delete"
                ]
            );
            server.abort();
        }
    }
}
