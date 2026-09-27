//! Desktop transport: execute the same API in process, without a TCP listener.
use crate::{AppState, config::ServiceConfig, router, state};
use anyhow::Result;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use bibi_core::Store;
use serde_json::Value;
use std::{fs::File, sync::atomic::Ordering};
use tower::ServiceExt;

pub struct LocalService {
    pub app: AppState,
    _lock: File,
    requests: tokio::sync::RwLock<()>,
}

impl LocalService {
    pub async fn open(config: ServiceConfig) -> Result<Self> {
        let token = config.prepare()?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(config.data_dir.join("service.lock"))?;
        lock.try_lock().map_err(|_| anyhow::anyhow!(
            "이 데이터 경로의 BiBi가 이미 실행 중입니다. 기존 앱을 사용하거나 실행 중인 서버에 연결하세요."
        ))?;
        let store = Store::open(config.data_dir.join("bibi.sqlite3"))?;
        let app = state(store, config, token);
        app.engine.start().await?;
        Ok(Self {
            app,
            _lock: lock,
            requests: tokio::sync::RwLock::new(()),
        })
    }

    pub async fn request(&self, path: &str, method: &str, value: Value) -> Result<(u16, Value)> {
        let _request = self.requests.read().await;
        if self.app.stopping.load(Ordering::Acquire) {
            anyhow::bail!("로컬 실행이 종료 중입니다.");
        }
        let body = if method == "POST" {
            serde_json::to_vec(&value)?
        } else {
            vec![]
        };
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {}", self.app.token))
            .header("content-type", "application/json")
            .body(Body::from(body))?;
        let response = router(self.app.clone()).oneshot(request).await?;
        let status = response.status().as_u16();
        let body = to_bytes(response.into_body(), 32 * 1024 * 1024).await?;
        Ok((status, serde_json::from_slice(&body)?))
    }

    pub async fn shutdown(&self) {
        let _requests = self.requests.write().await;
        self.app.stopping.store(true, Ordering::Release);
        self.app.engine.stop().await;
    }
}
