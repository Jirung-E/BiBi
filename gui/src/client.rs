use anyhow::{Context, Result, bail};
use bibi_server::{
    config::{ServiceConfig, default_data_dir, private_file},
    local::LocalService,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{path::PathBuf, time::Duration};
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
struct Connection {
    mode: String,
    url: String,
    token: String,
    server_id: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct ConnectionInfo {
    pub mode: String,
    pub url: String,
    pub managed_local: bool,
}
#[derive(Serialize, Debug)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}
impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self {
            status: 0,
            message: value.to_string(),
        }
    }
}
pub struct Desktop {
    data_dir: PathBuf,
    target: Mutex<Option<Connection>>,
    lifecycle: Mutex<()>,
    owned: Mutex<Option<std::sync::Arc<LocalService>>>,
    closing: AtomicBool,
}
fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn validated_url(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url)?;
    if !["http", "https"].contains(&parsed.scheme())
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("HTTP(S) 서버 주소가 필요합니다.");
    }
    Ok(url.trim_end_matches('/').into())
}
impl Desktop {
    pub fn new() -> Result<Self> {
        let data_dir = std::env::var_os("BIBI_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(default_data_dir);
        let target = if let Ok(url) = std::env::var("BIBI_SERVER") {
            let token = std::env::var("BIBI_TOKEN")
                .ok()
                .or_else(|| {
                    std::env::var_os("BIBI_TOKEN_FILE")
                        .and_then(|p| std::fs::read_to_string(p).ok())
                })
                .unwrap_or_default();
            Some(Connection {
                mode: "remote".into(),
                url: validated_url(&url)?,
                token: token.trim().into(),
                server_id: None,
            })
        } else if data_dir.join("connection.json").exists() {
            Some(serde_json::from_slice::<Connection>(&std::fs::read(
                data_dir.join("connection.json"),
            )?)?)
        } else {
            None
        };
        // A saved local connection is restarted as needed; a saved remote is never replaced by local.
        let target = target.filter(|connection| connection.mode == "remote");
        Ok(Self {
            data_dir,
            target: Mutex::new(target),
            lifecycle: Mutex::new(()),
            owned: Mutex::new(None),
            closing: AtomicBool::new(false),
        })
    }
    async fn snapshot(connection: &Connection) -> Result<Value> {
        let response = http()?
            .get(format!("{}/api/snapshot", connection.url))
            .bearer_auth(&connection.token)
            .send()
            .await?;
        if !response.status().is_success() {
            bail!(
                "서버 연결 {}. 주소와 인증 토큰을 확인하세요.",
                response.status()
            );
        }
        let value: Value = response.json().await?;
        let id = value["server_id"]
            .as_str()
            .context("BiBi 서버 응답이 아닙니다.")?;
        if connection
            .server_id
            .as_deref()
            .is_some_and(|expected| expected != id)
        {
            bail!("서버 ID가 바뀌었습니다. 설정에서 다시 연결하세요.");
        }
        Ok(value)
    }
    fn local_connection(&self) -> Result<Connection> {
        let info: Value =
            serde_json::from_slice(&std::fs::read(self.data_dir.join("service.json"))?)?;
        let url = info["url"]
            .as_str()
            .context("로컬 서버 주소가 없습니다.")?
            .replace("0.0.0.0", "127.0.0.1")
            .replace("[::]", "[::1]");
        let token = std::fs::read_to_string(self.data_dir.join("access-token"))?;
        Ok(Connection {
            mode: "local".into(),
            url: validated_url(&url)?,
            token: token.trim().into(),
            server_id: None,
        })
    }
    async fn start_local(&self) -> Result<Connection> {
        // An explicitly started server may already own this data directory.
        // Reuse it, but never spawn a server or bind a port from the desktop.
        if self.owned.lock().await.is_none() {
            if let Ok(mut connection) = self.local_connection()
                && let Ok(snapshot) = Self::snapshot(&connection).await
            {
                connection.mode = "local-server".into();
                connection.server_id = snapshot["server_id"].as_str().map(String::from);
                return Ok(connection);
            }
            let local = LocalService::open(ServiceConfig::new(self.data_dir.clone())).await?;
            *self.owned.lock().await = Some(std::sync::Arc::new(local));
        }
        let owned = self.owned.lock().await;
        let local = owned.as_ref().context("로컬 실행을 준비하지 못했습니다.")?;
        Ok(Connection {
            mode: "local".into(),
            url: String::new(),
            token: String::new(),
            server_id: Some(local.app.store.server_id()?),
        })
    }
    async fn connection(&self) -> Result<Connection> {
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_open()?;
        let mut target = self.target.lock().await;
        if target.is_none() {
            *target = Some(self.start_local().await?);
        }
        Ok(target.as_ref().unwrap().clone())
    }
    pub async fn info(&self) -> Result<ConnectionInfo> {
        let c = self.connection().await?;
        Ok(ConnectionInfo {
            mode: c.mode,
            url: c.url,
            managed_local: self.has_owned_server().await,
        })
    }
    pub async fn set(&self, url: String, token: String) -> Result<ConnectionInfo> {
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_open()?;
        let mut c = if url.trim().is_empty() {
            self.start_local().await?
        } else {
            Connection {
                mode: "remote".into(),
                url: validated_url(url.trim())?,
                token: token.trim().into(),
                server_id: None,
            }
        };
        if c.mode != "local" {
            let snapshot = Self::snapshot(&c).await?;
            c.server_id = snapshot["server_id"].as_str().map(String::from);
        }
        std::fs::create_dir_all(&self.data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.data_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let temp = self
            .data_dir
            .join(format!("connection-{}.tmp", uuid::Uuid::new_v4()));
        private_file(&temp, &serde_json::to_string(&c)?)?;
        let destination = self.data_dir.join("connection.json");
        #[cfg(windows)]
        if destination.exists() {
            std::fs::remove_file(&destination)?;
        }
        std::fs::rename(temp, destination)?;
        *self.target.lock().await = Some(c.clone());
        Ok(ConnectionInfo {
            mode: c.mode,
            url: c.url,
            managed_local: self.has_owned_server().await,
        })
    }
    fn ensure_open(&self) -> Result<()> {
        if self.closing.load(Ordering::Acquire) {
            bail!("BiBi가 종료 중입니다.");
        }
        Ok(())
    }
    async fn has_owned_server(&self) -> bool {
        self.owned.lock().await.is_some()
    }
    #[cfg(not(windows))]
    pub fn begin_shutdown(&self) -> bool {
        !self.closing.swap(true, Ordering::AcqRel)
    }

    #[cfg(windows)]
    pub fn instance_id(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        std::fs::create_dir_all(&self.data_dir)?;
        let path = self.data_dir.canonicalize()?;
        let explicit_server = std::env::var("BIBI_SERVER").ok();
        if explicit_server.is_none()
            && default_data_dir().canonicalize().ok().as_ref() == Some(&path)
        {
            // Preserve the installed app's existing WebView profile/preferences.
            return Ok("app.bibi.desktop".into());
        }
        let key = serde_json::to_vec(&(path, explicit_server))?;
        Ok(format!("app.bibi.desktop.p{:x}", Sha256::digest(key)))
    }

    #[cfg(windows)]
    pub async fn cached_info(&self) -> Option<ConnectionInfo> {
        let c = self.target.lock().await.clone()?;
        Some(ConnectionInfo {
            mode: c.mode,
            url: c.url,
            managed_local: self.has_owned_server().await,
        })
    }

    #[cfg(any(windows, test))]
    pub async fn owned_work_count(&self) -> Result<Option<usize>> {
        let _lifecycle = self.lifecycle.lock().await;
        if !self.has_owned_server().await {
            return Ok(None);
        }
        let owned = self.owned.lock().await;
        let value = serde_json::to_value(owned.as_ref().unwrap().app.store.snapshot()?)?;
        Ok(Some(
            value["runs"]
                .as_array()
                .context("실행 목록이 없습니다.")?
                .iter()
                .filter(|run| {
                    matches!(
                        run["state"].as_str(),
                        Some("queued" | "running" | "waiting_user" | "waiting_expert")
                    )
                })
                .count(),
        ))
    }

    pub async fn shutdown_owned(&self) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        self.closing.store(true, Ordering::Release);
        let mut owned = self.owned.lock().await;
        if let Some(server) = owned.as_mut() {
            server.shutdown().await;
        }
        *owned = None;
        Ok(())
    }
    pub async fn request(
        &self,
        path: String,
        method: String,
        body: Value,
    ) -> std::result::Result<Value, ApiError> {
        if !path.starts_with("/api/")
            || path.contains("..")
            || path.contains('#')
            || !matches!(method.as_str(), "GET" | "POST")
        {
            return Err(ApiError {
                status: 400,
                message: "허용되지 않은 BiBi API 요청입니다.".into(),
            });
        }
        let c = self.connection().await?;
        if c.mode == "local" {
            let local = self
                .owned
                .lock()
                .await
                .clone()
                .context("로컬 실행이 종료되었습니다.")?;
            let (status, value) = local.request(&path, &method, body).await?;
            return if (200..300).contains(&status) {
                Ok(value)
            } else {
                Err(ApiError {
                    status,
                    message: value["error"].as_str().unwrap_or("로컬 처리 오류").into(),
                })
            };
        }
        let client = http()?;
        let url = format!("{}{path}", c.url);
        let request = if method == "POST" {
            client.post(url).json(&body)
        } else {
            client.get(url)
        };
        let response = request
            .bearer_auth(c.token)
            .send()
            .await
            .map_err(|e| ApiError {
                status: 0,
                message: e.to_string(),
            })?;
        let status = response.status();
        let value: Value = response.json().await.map_err(|e| ApiError {
            status: 0,
            message: e.to_string(),
        })?;
        if !status.is_success() {
            return Err(ApiError {
                status: status.as_u16(),
                message: value["error"].as_str().unwrap_or("서버 응답 오류").into(),
            });
        }
        if path == "/api/snapshot"
            && c.server_id
                .as_deref()
                .is_some_and(|id| value["server_id"].as_str() != Some(id))
        {
            return Err(ApiError {
                status: 409,
                message: "서버 ID가 바뀌었습니다. 설정에서 다시 연결하세요.".into(),
            });
        }
        Ok(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_urls_and_redirect_targets_are_not_accepted() {
        assert!(validated_url("https://u:secret@host").is_err());
        assert!(validated_url("file:///tmp/test").is_err());
        assert!(validated_url("https://host?token=secret").is_err());
        assert_eq!(validated_url("https://host/").unwrap(), "https://host");
    }
    #[tokio::test]
    async fn failed_remote_switch_keeps_previous_target_and_tokens_stay_native() {
        let dir = tempfile::tempdir().unwrap();
        let remote = Connection {
            mode: "remote".into(),
            url: "http://127.0.0.1:1".into(),
            token: "private-token".into(),
            server_id: None,
        };
        let desktop = Desktop {
            data_dir: dir.path().into(),
            target: Mutex::new(Some(remote)),
            lifecycle: Mutex::new(()),
            owned: Mutex::new(None),
            closing: AtomicBool::new(false),
        };
        assert!(
            desktop
                .set("file:///invalid".into(), "new-token".into())
                .await
                .is_err()
        );
        assert_eq!(desktop.info().await.unwrap().url, "http://127.0.0.1:1");
        assert!(
            !serde_json::to_string(&desktop.info().await.unwrap())
                .unwrap()
                .contains("private-token")
        );
        assert!(
            desktop
                .request("https://other.invalid/".into(), "GET".into(), Value::Null)
                .await
                .is_err()
        );
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;
    #[tokio::test]
    async fn native_proxy_preserves_http_contract_and_server_identity() {
        let dir = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let app = bibi_server::state(
            bibi_core::Store::memory().unwrap(),
            bibi_server::config::ServiceConfig::new(dir.path().into()),
            "native-private-token-with-32-characters".into(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server =
            tokio::spawn(axum::serve(listener, bibi_server::router(app.clone())).into_future());
        let desktop = Desktop {
            data_dir: data.path().into(),
            target: Mutex::new(None),
            lifecycle: Mutex::new(()),
            owned: Mutex::new(None),
            closing: AtomicBool::new(false),
        };
        desktop
            .set(url.clone(), app.token.as_ref().clone())
            .await
            .unwrap();
        let project=desktop.request("/api/command".into(),"POST".into(),serde_json::json!({"type":"create_project","name":"native","workspace":dir.path(),"constraints":[]})).await.unwrap();
        let snapshot = desktop
            .request("/api/snapshot".into(), "GET".into(), Value::Null)
            .await
            .unwrap();
        assert_eq!(snapshot["projects"][0]["id"], project["id"]);
        assert!(!snapshot.to_string().contains(app.token.as_str()));
        assert!(
            desktop
                .set("http://127.0.0.1:1".into(), "another-token".into())
                .await
                .is_err()
        );
        assert_eq!(desktop.info().await.unwrap().url, url);
        app.store
            .set_setting("server_id", &"changed-server")
            .unwrap();
        assert_eq!(
            desktop
                .request("/api/snapshot".into(), "GET".into(), Value::Null)
                .await
                .unwrap_err()
                .status,
            409
        );
        server.abort();
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;
    fn desktop(dir: &std::path::Path) -> Desktop {
        Desktop {
            data_dir: dir.into(),
            target: Mutex::new(None),
            lifecycle: Mutex::new(()),
            owned: Mutex::new(None),
            closing: AtomicBool::new(false),
        }
    }
    #[tokio::test]
    async fn desktop_uses_no_listener_or_server_process_and_releases_its_store() {
        let dir = tempfile::tempdir().unwrap();
        let owner = desktop(dir.path());
        let info = owner.info().await.unwrap();
        assert_eq!(info.mode, "local");
        assert_eq!(info.url, "");
        assert!(info.managed_local);
        assert!(!dir.path().join("service.json").exists());
        assert!(desktop(dir.path()).info().await.is_err());
        let first = owner
            .request("/api/snapshot".into(), "GET".into(), Value::Null)
            .await
            .unwrap();
        assert_eq!(owner.owned_work_count().await.unwrap(), Some(0));
        owner.shutdown_owned().await.unwrap();
        assert!(owner.info().await.is_err());
        let next = desktop(dir.path());
        let restored = next
            .request("/api/snapshot".into(), "GET".into(), Value::Null)
            .await
            .unwrap();
        assert_eq!(first["server_id"], restored["server_id"]);
        next.shutdown_owned().await.unwrap();
    }
    #[tokio::test]
    async fn explicit_server_is_reused_and_survives_desktop_exit() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = ServiceConfig::new(dir.path().into());
        config.bind = "127.0.0.1:0".parse().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(bibi_server::serve_with_shutdown(config, async {
            let _ = stopped.await;
        }));
        tokio::time::timeout(Duration::from_secs(5), async {
            while !dir.path().join("service.json").exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let client = desktop(dir.path());
        let info = client.info().await.unwrap();
        assert_eq!(info.mode, "local-server");
        assert!(!info.managed_local);
        let connection = client.connection().await.unwrap();
        let before = Desktop::snapshot(&connection).await.unwrap();
        client.shutdown_owned().await.unwrap();
        assert_eq!(
            Desktop::snapshot(&connection).await.unwrap()["server_id"],
            before["server_id"]
        );
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn local_work_survives_connection_switch_and_is_stopped_on_exit() {
        let dir = tempfile::tempdir().unwrap();
        let client = desktop(dir.path());
        client.info().await.unwrap();
        let local = client.owned.lock().await.clone().unwrap();
        let workspace = dir.path().to_string_lossy().into_owned();
        client.request("/api/command".into(),"POST".into(),serde_json::json!({"type":"create_project","name":"local","workspace":workspace,"constraints":[]})).await.unwrap();
        let snapshot = local.app.store.snapshot().unwrap();
        let receipt = local
            .app
            .store
            .submit(bibi_core::Submission {
                submission_id: "local-work".into(),
                project_key: snapshot.projects[0].id.clone(),
                work_id: None,
                title: None,
                question: "fixture ".repeat(300),
                provider: bibi_core::Provider::Mock,
                provider_id: None,
                model: "mock".into(),
                host_id: "local".into(),
                role: "test".into(),
                mode: bibi_core::SubmitMode::Fresh,
                target_run_id: None,
                expected_turn_id: None,
                expected_context_revision: None,
                read_only: true,
                approval_mode: None,
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while local
                .app
                .store
                .run(&receipt.run_id)
                .unwrap()
                .turn_id
                .is_none()
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let remote_dir = tempfile::tempdir().unwrap();
        let remote = bibi_server::state(
            bibi_core::Store::memory().unwrap(),
            ServiceConfig::new(remote_dir.path().into()),
            "switch-test-token-at-least-32-characters".into(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(axum::serve(listener, bibi_server::router(remote)).into_future());
        client
            .set(url, "switch-test-token-at-least-32-characters".into())
            .await
            .unwrap();
        assert_eq!(client.owned_work_count().await.unwrap(), Some(1));
        let restored = client.set(String::new(), String::new()).await.unwrap();
        assert_eq!(restored.mode, "local");
        assert_eq!(restored.url, "");
        assert_eq!(local.app.store.snapshot().unwrap().runs.len(), 1);
        client.shutdown_owned().await.unwrap();
        assert_eq!(
            local.app.store.run(&receipt.run_id).unwrap().state,
            bibi_core::RunState::Interrupted
        );
        assert!(
            local
                .request("/api/snapshot", "GET", Value::Null)
                .await
                .is_err()
        );
        assert!(!dir.path().join("service.json").exists());
        server.abort();
    }
    #[tokio::test]
    async fn failed_local_start_does_not_leave_an_owner_or_listener() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("access-token"), "invalid").unwrap();
        let client = desktop(dir.path());
        assert!(client.info().await.is_err());
        assert!(!client.has_owned_server().await);
        assert!(!dir.path().join("service.json").exists());
    }
}
