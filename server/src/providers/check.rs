//! Read-only probes for unsaved provider drafts. Never create model turns or save state.
use super::rpc::Rpc;
use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::{Provider, ProviderConfig};
use serde::Serialize;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

const MAX_RESPONSE: usize = 1024 * 1024;
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize)]
pub struct ConnectionCheck {
    pub ok: bool,
    pub message: String,
    pub models: Vec<String>,
}

pub fn endpoint(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value.trim()).context("API 주소가 올바르지 않습니다.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("API 주소에는 http(s) 주소를 입력하고 키는 별도 항목에 입력하세요.");
    }
    Ok(url)
}

pub async fn check(
    engine: &Engine,
    provider: &ProviderConfig,
    api_key: Option<String>,
) -> ConnectionCheck {
    let result = tokio::time::timeout(CHECK_TIMEOUT, probe(engine, provider, api_key))
        .await
        .unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "연결 확인 시간이 초과되었습니다. 서버 주소·실행 파일을 확인하세요."
            ))
        });
    match result {
        Ok((message, models)) => ConnectionCheck {
            ok: true,
            message,
            models,
        },
        Err(error) => ConnectionCheck {
            ok: false,
            message: error.to_string(),
            models: vec![],
        },
    }
}

async fn probe(
    engine: &Engine,
    provider: &ProviderConfig,
    api_key: Option<String>,
) -> Result<(String, Vec<String>)> {
    if provider.host_id != "local" || provider.remote_id.is_some() {
        bail!("원격 제공자는 해당 BiBi 호스트에서 확인하세요.");
    }
    if provider.args.len() > 64 || provider.args.iter().any(|a| a.len() > 8192) {
        bail!("실행 인자가 너무 큽니다.");
    }
    match provider.adapter {
        Provider::Ollama | Provider::OpenAi => api(engine, provider, api_key).await,
        Provider::Codex => {
            require_command(provider)?;
            let mut config = engine.config.clone();
            config.codex_command = provider.command.trim().into();
            config.codex_args = provider.args.clone();
            let mut rpc = Rpc::connect(&config).await.map_err(|error| {
                if error.downcast_ref::<super::launch::LaunchError>().is_some() {
                    error
                } else {
                    anyhow::anyhow!("Codex를 실행했지만 제어 연결에 실패했습니다. CLI 버전·실행 인자·설정 파일을 확인하세요.")
                }
            })?;
            let account = rpc
                .request("account/read", json!({"refreshToken":false}))
                .await
                .map_err(|_| anyhow::anyhow!("Codex 로그인 상태를 확인할 수 없습니다."))?;
            if account["account"].is_null() && account["requiresOpenaiAuth"] != false {
                bail!("Codex 로그인이 필요합니다. BiBi 서버가 실행되는 컴퓨터에서 로그인하세요.");
            }
            Ok(("Codex 제어 연결·인증 설정 확인".into(), vec![]))
        }
        Provider::Claude => {
            require_command(provider)?;
            let mut command = super::launch::command(provider.command.trim())?;
            command
                .args(&provider.args)
                .args(["auth", "status", "--json"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let mut child = super::launch::spawn(&mut command)?;
            let mut bytes = Vec::new();
            child
                .stdout
                .take()
                .context("Claude Code 응답 없음")?
                .take((MAX_RESPONSE + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .context("Claude Code 응답을 읽을 수 없습니다.")?;
            if bytes.len() > MAX_RESPONSE {
                bail!("Claude Code 응답이 너무 큽니다.");
            }
            let status = child.wait().await.context("Claude Code 상태 확인 실패")?;
            let data: Value = serde_json::from_slice(&bytes)
                .context("Claude Code 로그인 응답 형식이 올바르지 않습니다.")?;
            if !status.success() || data["loggedIn"] != true {
                bail!(
                    "Claude Code 로그인이 필요합니다. BiBi 서버가 실행되는 컴퓨터에서 claude auth login을 실행하세요."
                );
            }
            Ok(("Claude Code 실행·로그인 상태 확인".into(), vec![]))
        }
        Provider::Command | Provider::Mock => {
            bail!("이 연결 방식은 모델 호출 없는 연결 확인을 지원하지 않습니다.")
        }
    }
}

fn require_command(provider: &ProviderConfig) -> Result<()> {
    if provider.command.trim().is_empty() {
        bail!("실행 파일을 입력하세요.");
    }
    Ok(())
}

async fn api(
    engine: &Engine,
    provider: &ProviderConfig,
    api_key: Option<String>,
) -> Result<(String, Vec<String>)> {
    let url = endpoint(&provider.endpoint)?;
    // A draft changing destination must not silently send a stored secret there.
    let key = match api_key {
        Some(value) => Some(value),
        None => match engine.store.provider_secret(&provider.id)? {
            Some(value) => {
                let saved = engine.store.provider(&provider.id)?;
                if saved.adapter != provider.adapter
                    || saved.host_id != "local"
                    || endpoint(&saved.endpoint)? != url
                {
                    bail!(
                        "API 주소를 바꾼 경우 확인할 API 키를 다시 입력하거나 저장된 키 제거를 선택하세요."
                    );
                }
                Some(value)
            }
            None => None,
        },
    };
    let base = url.as_str().trim_end_matches('/');
    let target = if provider.adapter == Provider::Ollama {
        format!("{base}/api/tags")
    } else {
        format!(
            "{}/models",
            base.strip_suffix("/chat/completions").unwrap_or(base)
        )
    };
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut request = client.get(target);
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await.map_err(|e| if e.is_timeout() {
        anyhow::anyhow!("API 연결 시간이 초과되었습니다.")
    } else {
        anyhow::anyhow!("API에 연결할 수 없습니다. BiBi 서버에서 접근 가능한 주소와 서비스 실행 상태를 확인하세요.")
    })?;
    let status = response.status();
    if status.is_redirection() {
        bail!("API 주소가 다른 곳으로 이동됩니다. 최종 API 주소를 입력하세요.");
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        bail!(
            "API 인증 실패 (HTTP {}). API 키·접근 권한을 확인하세요.",
            status.as_u16()
        );
    }
    if !status.is_success() {
        bail!(
            "모델 목록 조회 실패 (HTTP {}). 주소와 모델 목록 API 지원 여부를 확인하세요.",
            status.as_u16()
        );
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("API 응답을 읽을 수 없습니다.")?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE {
            bail!("API 응답이 너무 큽니다.");
        }
        bytes.extend_from_slice(&chunk);
    }
    let data: Value =
        serde_json::from_slice(&bytes).context("모델 목록 JSON 응답이 올바르지 않습니다.")?;
    let (field, name) = if provider.adapter == Provider::Ollama {
        ("models", "name")
    } else {
        ("data", "id")
    };
    let entries = data[field]
        .as_array()
        .context("모델 목록 응답 형식이 올바르지 않습니다.")?;
    let mut models = Vec::new();
    for item in entries {
        let value = item[name]
            .as_str()
            .filter(|v| !v.trim().is_empty())
            .context("모델 목록에 올바른 이름이 없습니다.")?;
        if value.len() > 256 {
            bail!("모델 이름이 너무 깁니다.");
        }
        models.push(value.to_owned());
    }
    models.sort();
    models.dedup();
    let message = if models.is_empty() {
        "API 연결 확인 · 설치/등록된 모델 없음".into()
    } else {
        format!("API 연결 확인 · 모델 {}개", models.len())
    };
    // Match the editable provider model-list limit; the message retains the total count.
    models.truncate(200);
    Ok((message, models))
}
