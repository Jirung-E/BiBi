use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub frontend: Option<PathBuf>,
    pub public_origin: Option<String>,
    pub codex_command: String,
    #[serde(default = "default_claude")]
    pub claude_command: String,
    pub ollama_url: String,
}
fn default_claude() -> String {
    "claude".into()
}
impl ServiceConfig {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            bind: "127.0.0.1:44880".parse().unwrap(),
            frontend: None,
            public_origin: None,
            codex_command: "codex".into(),
            claude_command: default_claude(),
            ollama_url: "http://127.0.0.1:11434".into(),
        }
    }
    pub fn prepare(&self) -> Result<String> {
        fs::create_dir_all(&self.data_dir)?;
        private_directory(&self.data_dir)?;
        if let Some(origin) = &self.public_origin {
            let url = reqwest::Url::parse(origin)?;
            if url.scheme() != "https" || url.host_str().is_none() || url.path() != "/" {
                bail!("public-origin에는 HTTPS 출처만 지정할 수 있습니다.");
            }
        }
        let path = self.data_dir.join("access-token");
        if !path.exists() {
            let secret = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            match private_file(&path, &secret) {
                Ok(()) => (),
                Err(e) if path.exists() => {
                    tracing::debug!("Token created by another service: {e}");
                }
                Err(e) => return Err(e),
            }
        }
        let token = fs::read_to_string(&path)
            .context("인증 토큰 파일을 읽을 수 없습니다.")?
            .trim()
            .to_owned();
        if token.len() < 32 {
            bail!("인증 토큰이 너무 짧습니다.");
        }
        Ok(token)
    }
}
pub fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bibi")
}
pub fn private_file(path: &Path, value: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(value.as_bytes())?;
    file.sync_all()?;
    Ok(())
}
fn private_directory(_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
