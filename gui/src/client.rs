use crate::server_lifetime::{ManagedServer, StartingServer};
use anyhow::{Context, Result, bail};
use bibi_server::config::{default_data_dir, private_file};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
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
    cli: PathBuf,
    target: Mutex<Option<Connection>>,
    lifecycle: Mutex<()>,
    owned: Mutex<Option<ManagedServer>>,
    closing: AtomicBool,
    manage_local: bool,
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
        let cli = std::env::current_exe()?
            .parent()
            .context("앱 실행 경로 없음")?
            .join(if cfg!(windows) { "bibi.exe" } else { "bibi" });
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
        let target = target.filter(|connection| connection.mode != "local");
        Ok(Self {
            data_dir,
            cli,
            target: Mutex::new(target),
            lifecycle: Mutex::new(()),
            owned: Mutex::new(None),
            closing: AtomicBool::new(false),
            manage_local: cfg!(windows),
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
        if let Ok(mut connection) = self.local_connection()
            && let Ok(snapshot) = Self::snapshot(&connection).await
        {
            connection.server_id = snapshot["server_id"].as_str().map(String::from);
            return Ok(connection);
        }
        if !self.cli.is_file() {
            bail!(
                "함께 설치된 bibi 실행 파일을 찾을 수 없습니다: {}",
                self.cli.display()
            );
        }
        let mut command = Command::new(&self.cli);
        command
            .arg("--data-dir")
            .arg(&self.data_dir)
            .args(["server", "--bind", "127.0.0.1:0"])
            .env_remove("BIBI_SERVER")
            .env_remove("BIBI_TOKEN")
            .env_remove("BIBI_TOKEN_FILE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut paths = vec![self.cli.parent().unwrap().to_path_buf()];
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".local/bin"));
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            paths.push(PathBuf::from(appdata).join("npm"));
        }
        if cfg!(unix) {
            paths.extend([
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
            ]);
        }
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        command.env("PATH", std::env::join_paths(paths)?);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000 | 0x00000200);
        }
        if self.manage_local {
            command.arg("--desktop-managed").stdin(Stdio::piped());
        }
        let mut starting = StartingServer(Some(command.spawn()?));
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if let Ok(mut connection) = self.local_connection()
                    && let Ok(snapshot) = Self::snapshot(&connection).await
                {
                    connection.server_id = snapshot["server_id"].as_str().map(String::from);
                    return Ok(connection);
                }
                if let Some(status) = starting.0.as_mut().unwrap().try_wait()? {
                    bail!("BiBi 서비스가 시작되지 않았습니다: {status}");
                }
            }
        })
        .await
        .context("BiBi 서비스 시작을 확인하지 못했습니다. 다시 연결하세요.")??;
        let service: Value =
            serde_json::from_slice(&std::fs::read(self.data_dir.join("service.json"))?)?;
        if service["pid"].as_u64() == Some(u64::from(starting.0.as_ref().unwrap().id())) {
            let child = starting.0.take().unwrap();
            if self.manage_local {
                *self.owned.lock().await = Some(ManagedServer { child });
            } else {
                // Other platforms retain the independent service lifetime.
                // Reap it if it exits while this desktop is still running.
                std::thread::spawn(move || {
                    let mut child = child;
                    let _ = child.wait();
                });
            }
        }
        Ok(result)
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
        let snapshot = Self::snapshot(&c).await?;
        c.server_id = snapshot["server_id"].as_str().map(String::from);
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
        self.owned
            .lock()
            .await
            .as_mut()
            .is_some_and(ManagedServer::is_running)
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
        // This always reads the local data directory, even after switching the
        // UI to a remote server. Never send a shutdown to the selected URL.
        let value = Self::snapshot(&self.local_connection()?).await?;
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

    #[cfg(any(windows, test))]
    pub async fn shutdown_owned(&self) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        self.closing.store(true, Ordering::Release);
        let mut owned = self.owned.lock().await;
        if let Some(server) = owned.as_mut() {
            server.shutdown().await?;
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
            cli: PathBuf::from("missing"),
            target: Mutex::new(Some(remote)),
            lifecycle: Mutex::new(()),
            owned: Mutex::new(None),
            closing: AtomicBool::new(false),
            manage_local: true,
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
            cli: PathBuf::from("unused"),
            target: Mutex::new(None),
            lifecycle: Mutex::new(()),
            owned: Mutex::new(None),
            closing: AtomicBool::new(false),
            manage_local: true,
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
        let cli = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(if cfg!(windows) { "bibi.exe" } else { "bibi" });
        assert!(
            cli.is_file(),
            "Run just build-debug before desktop lifecycle tests"
        );
        Desktop {
            data_dir: dir.into(),
            cli,
            target: Mutex::new(None),
            lifecycle: Mutex::new(()),
            owned: Mutex::new(None),
            closing: AtomicBool::new(false),
            manage_local: true,
        }
    }

    #[tokio::test]
    async fn managed_server_reuses_connection_and_exits_without_stopping_another_desktop_server() {
        let dir = tempfile::tempdir().unwrap();
        let owner = desktop(dir.path());
        let first = owner.info().await.unwrap();
        assert!(first.managed_local);
        let attached = desktop(dir.path());
        assert_eq!(attached.info().await.unwrap().url, first.url);
        assert!(!attached.info().await.unwrap().managed_local);
        assert_eq!(attached.owned_work_count().await.unwrap(), None);
        attached.shutdown_owned().await.unwrap();
        assert!(
            owner
                .request("/api/snapshot".into(), "GET".into(), Value::Null)
                .await
                .is_ok()
        );
        assert_eq!(owner.owned_work_count().await.unwrap(), Some(0));
        owner.shutdown_owned().await.unwrap();
        assert!(
            http()
                .unwrap()
                .get(format!("{}/health", first.url))
                .send()
                .await
                .is_err()
        );
        assert!(
            owner.info().await.is_err(),
            "a pending frontend request must not restart a closing server"
        );
        let restarted = desktop(dir.path());
        assert!(restarted.info().await.unwrap().managed_local);
        restarted.shutdown_owned().await.unwrap();
    }

    #[tokio::test]
    async fn remote_switch_still_stops_only_the_owned_local_server() {
        let local_dir = tempfile::tempdir().unwrap();
        let remote_dir = tempfile::tempdir().unwrap();
        let local = desktop(local_dir.path());
        let remote = desktop(remote_dir.path());
        let local_info = local.info().await.unwrap();
        let remote_info = remote.info().await.unwrap();
        let token = std::fs::read_to_string(remote_dir.path().join("access-token")).unwrap();
        let switched = local.set(remote_info.url.clone(), token).await.unwrap();
        assert_eq!(switched.mode, "remote");
        assert!(switched.managed_local);
        local.shutdown_owned().await.unwrap();
        assert!(
            http()
                .unwrap()
                .get(format!("{}/health", local_info.url))
                .send()
                .await
                .is_err()
        );
        assert!(
            remote
                .request("/api/snapshot".into(), "GET".into(), Value::Null)
                .await
                .is_ok()
        );
        remote.shutdown_owned().await.unwrap();
    }

    #[tokio::test]
    async fn failed_managed_start_does_not_keep_a_child() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("access-token"), "invalid").unwrap();
        let client = desktop(dir.path());
        assert!(client.info().await.is_err());
        assert!(!client.has_owned_server().await);
        assert!(!dir.path().join("service.json").exists());
    }
}
