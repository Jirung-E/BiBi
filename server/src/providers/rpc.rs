use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{collections::VecDeque, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};
pub struct Rpc {
    _child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    pending: VecDeque<Value>,
    next_id: u64,
}
impl Rpc {
    pub async fn connect(config: &crate::config::ServiceConfig) -> Result<Self> {
        Self::start(config, false).await
    }
    pub async fn connect_tools(config: &crate::config::ServiceConfig) -> Result<Self> {
        Self::start(config, true).await
    }
    async fn start(config: &crate::config::ServiceConfig, experimental: bool) -> Result<Self> {
        let binary = &config.codex_command;
        let mut paths = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .collect::<Vec<_>>();
        if let Some(parent) = std::env::current_exe()?.parent() {
            paths.insert(0, parent.into());
        }
        let path = std::env::join_paths(paths)?;
        let mut child = Command::new(binary)
            .args(&config.codex_args)
            .args(["app-server", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("BIBI_DATA_DIR", &config.data_dir)
            .env_remove("BIBI_SERVER")
            .env_remove("BIBI_TOKEN")
            .env_remove("BIBI_TOKEN_FILE")
            .env("PATH", path)
            .kill_on_drop(true)
            .spawn()
            .context("Codex App Server를 시작할 수 없습니다. Codex 설치·로그인을 확인하세요.")?;
        let stdin = child.stdin.take().context("Codex stdin 없음")?;
        let lines = BufReader::new(child.stdout.take().context("Codex stdout 없음")?).lines();
        let mut rpc = Self {
            _child: child,
            stdin,
            lines,
            pending: VecDeque::new(),
            next_id: 0,
        };
        rpc.request("initialize",json!({"clientInfo":{"name":"bibi","title":"BiBi","version":bibi_core::VERSION},"capabilities":{"experimentalApi":experimental}})).await?;
        rpc.send(json!({"method":"initialized"})).await?;
        Ok(rpc)
    }
    pub fn alive(&mut self) -> bool {
        self._child.try_wait().is_ok_and(|v| v.is_none())
    }
    pub async fn send(&mut self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.stdin.write_all(&bytes).await?;
        self.stdin.flush().await?;
        Ok(())
    }
    async fn line(&mut self) -> Result<Value> {
        let line = self
            .lines
            .next_line()
            .await?
            .context("Codex 연결이 종료되었습니다.")?;
        if line.len() > 16 * 1024 * 1024 {
            bail!("Codex 응답 프레임이 너무 큽니다.");
        }
        Ok(serde_json::from_str(&line)?)
    }
    pub async fn next(&mut self) -> Result<Value> {
        if let Some(v) = self.pending.pop_front() {
            Ok(v)
        } else {
            self.line().await
        }
    }
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"id":id,"method":method,"params":params}))
            .await?;
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let value = self.line().await?;
                if value["id"].as_u64() == Some(id) && value.get("method").is_none() {
                    if let Some(error) = value.get("error") {
                        bail!(
                            "{method}: {}",
                            error["message"].as_str().unwrap_or("Codex 요청 실패")
                        );
                    }
                    return Ok(value["result"].clone());
                }
                self.pending.push_back(value);
                if self.pending.len() > 10000 {
                    bail!("Codex 알림 대기열 초과");
                }
            }
        })
        .await
        .context("Codex 응답 시간 초과")?
    }
}
