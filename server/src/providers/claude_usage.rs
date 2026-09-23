use crate::runtime::Engine;
use anyhow::{Context, Result, bail};
use bibi_core::*;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn config_directory() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Ok(path.into());
    }
    Ok(dirs::home_dir()
        .context("사용자 홈 경로를 찾을 수 없습니다.")?
        .join(".claude"))
}

pub fn access_token(value: &Value) -> Option<String> {
    value["claudeAiOauth"]["accessToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
}

async fn credential() -> Result<String> {
    let path = config_directory()?.join(".credentials.json");
    if let Ok(bytes) = tokio::fs::read(path).await
        && let Some(token) = access_token(&serde_json::from_slice(&bytes)?)
    {
        return Ok(token);
    }
    #[cfg(target_os = "macos")]
    {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new("/usr/bin/security")
                .args([
                    "find-generic-password",
                    "-s",
                    "Claude Code-credentials",
                    "-w",
                ])
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        if output.status.success()
            && let Some(token) = access_token(&serde_json::from_slice(&output.stdout)?)
        {
            return Ok(token);
        }
    }
    bail!(
        "Claude 구독 로그인 정보를 읽을 수 없습니다. 이 호스트에서 Claude Code 로그인을 확인하세요."
    )
}

pub fn windows(data: &Value) -> Vec<QuotaWindow> {
    [
        ("five_hour", "5시간", Some(300)),
        ("seven_day", "주간", Some(10080)),
        ("seven_day_sonnet", "Sonnet 주간", Some(10080)),
        ("seven_day_opus", "Opus 주간", Some(10080)),
        ("seven_day_oauth_apps", "연결 앱 주간", Some(10080)),
    ]
    .into_iter()
    .filter_map(|(key, label, duration)| {
        let window = &data[key];
        let used = window["utilization"].as_f64().filter(|v| v.is_finite())?;
        let resets_at = window["resets_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.timestamp_millis());
        Some(QuotaWindow {
            label: label.into(),
            remaining_percent: Some((100.0 - used).clamp(0.0, 100.0)),
            duration_minutes: duration,
            resets_at,
        })
    })
    .collect()
}

pub async fn refresh(engine: &Engine) -> Result<()> {
    let token = credential().await?;
    // The installed Claude Code client uses this first-party OAuth usage endpoint.
    // Do not redirect the subscription credential to a provider-configured URL.
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .build()?
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Claude 구독 한도 조회 HTTP {}", response.status());
    }
    let data: Value = response.json().await?;
    let windows = windows(&data);
    if windows.is_empty() {
        bail!("이 Claude 계정이 구독 한도 수치를 제공하지 않았습니다.");
    }
    let mut quota = engine
        .store
        .quotas()?
        .into_iter()
        .find(|q| q.id == engine.quota_id("Claude"))
        .unwrap_or(Quota {
            id: engine.quota_id("Claude"),
            provider: Provider::Claude,
            provider_id: engine.provider_id(),
            account: "Claude Code 로그인 계정".into(),
            host_id: "local".into(),
            model: None,
            status: "unknown".into(),
            windows: vec![],
            observed_at: None,
            reason: None,
        });
    quota.windows = windows;
    quota.status = "known".into();
    quota.observed_at = Some(now());
    quota.reason = None;
    engine.store.upsert_quota(quota)?;
    Ok(())
}

pub async fn session_file_in(root: &Path, session: &str) -> Option<String> {
    uuid::Uuid::parse_str(session).ok()?;
    let mut directories = tokio::fs::read_dir(root.join("projects")).await.ok()?;
    while let Ok(Some(directory)) = directories.next_entry().await {
        let file = directory.path().join(format!("{session}.jsonl"));
        if tokio::fs::metadata(&file).await.is_ok_and(|m| m.is_file()) {
            return Some(file.to_string_lossy().into());
        }
    }
    None
}

pub async fn session_file(session: &str) -> Option<String> {
    session_file_in(&config_directory().ok()?, session).await
}
