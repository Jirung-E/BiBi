use anyhow::{Context, Result, bail};
use bibi_core::*;
use bibi_server::{
    Command,
    config::{ServiceConfig, default_data_dir},
};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};

#[derive(Parser)]
#[command(name = "bibi", version, about = "BiBi 세션 캔버스")]
struct Args {
    #[arg(long, global = true, env = "BIBI_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true, env = "BIBI_SERVER")]
    server: Option<String>,
    #[arg(long, global = true, env = "BIBI_TOKEN_FILE")]
    token_file: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
    Server {
        /// Internal desktop lifetime pipe. EOF requests a graceful shutdown.
        #[arg(long, hide = true)]
        desktop_managed: bool,
        #[arg(long, default_value = "127.0.0.1:44880")]
        bind: std::net::SocketAddr,
        #[arg(long)]
        frontend: Option<PathBuf>,
        #[arg(long)]
        public_origin: Option<String>,
        #[arg(long, default_value = "codex")]
        codex_command: String,
        #[arg(long, default_value = "claude")]
        claude_command: String,
        #[arg(long, default_value = "http://127.0.0.1:11434")]
        ollama_url: String,
    },
    Status,
    Host {
        name: String,
        url: String,
        #[arg(long)]
        project: String,
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        guild: Option<String>,
        #[arg(long)]
        peer_token_file: PathBuf,
    },
    Runs {
        #[arg(long)]
        project: Option<String>,
    },
    Show {
        run_id: String,
    },
    Projects,
    Project {
        name: String,
        workspace: PathBuf,
        #[arg(long)]
        guild: Option<PathBuf>,
    },
    Ask {
        question: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long, default_value = "")]
        model: String,
        #[arg(long, default_value = "업무 조정")]
        role: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        read_only: bool,
        #[arg(long, requires = "from")]
        steer: bool,
        #[arg(long, conflicts_with = "steer")]
        fresh: bool,
        #[arg(long)]
        submission_id: Option<String>,
    },
    Inbox {
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        response: Option<String>,
    },
    Wait {
        run_id: String,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
    },
    Interrupt {
        run_id: String,
    },
    Resolve {
        run_id: String,
        #[arg(long)]
        confirmed_stopped: bool,
    },
    Context {
        work_id: String,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    Command {
        file: PathBuf,
    },
    Events {
        #[arg(long, default_value_t = 0)]
        after: i64,
    },
    Auth,
    Guild {
        #[arg(long)]
        project: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
}
struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
}
impl Client {
    fn new(args: &Args) -> Result<Self> {
        let dir = args.data_dir.clone().unwrap_or_else(default_data_dir);
        let url = if let Some(server) = &args.server {
            server.clone()
        } else {
            let path = dir.join("service.json");
            if path.exists() {
                let info: Value = serde_json::from_slice(&std::fs::read(path)?)?;
                let value = info["url"].as_str().context("서버 주소가 없습니다.")?;
                value
                    .replace("0.0.0.0", "127.0.0.1")
                    .replace("[::]", "[::1]")
            } else {
                "http://127.0.0.1:44880".into()
            }
        };
        let token = std::env::var("BIBI_TOKEN")
            .ok()
            .or_else(|| {
                std::fs::read_to_string(
                    args.token_file
                        .clone()
                        .unwrap_or_else(|| dir.join("access-token")),
                )
                .ok()
            })
            .context("인증 정보가 없습니다. bibi server를 실행하거나 --token-file을 지정하세요.")?;
        let parsed = reqwest::Url::parse(&url)?;
        if !["http", "https"].contains(&parsed.scheme())
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
        {
            bail!("올바른 HTTP(S) 서버 주소가 필요합니다.");
        }
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            url: url.trim_end_matches('/').into(),
            token: token.trim().into(),
        })
    }
    async fn get(&self, path: &str) -> Result<Value> {
        self.decode(
            self.http
                .get(format!("{}{path}", self.url))
                .bearer_auth(&self.token)
                .send()
                .await?,
        )
        .await
    }
    async fn command(&self, command: Command) -> Result<Value> {
        self.decode(
            self.http
                .post(format!("{}/api/command", self.url))
                .bearer_auth(&self.token)
                .json(&command)
                .send()
                .await?,
        )
        .await
    }
    async fn decode(&self, response: reqwest::Response) -> Result<Value> {
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .context("서버의 JSON 응답을 읽을 수 없습니다.")?;
        if !status.is_success() {
            bail!(
                "{}: {}",
                status,
                value["error"].as_str().unwrap_or("서버 오류")
            );
        }
        Ok(value)
    }
}
fn print(value: Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bibi=info,bibi_server=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    if let Some(Action::Server {
        desktop_managed,
        bind,
        frontend,
        public_origin,
        codex_command,
        claude_command,
        ollama_url,
    }) = &args.command
    {
        let mut config = ServiceConfig::new(args.data_dir.clone().unwrap_or_else(default_data_dir));
        config.bind = *bind;
        if let Some(frontend) = frontend {
            config.frontend = Some(frontend.clone());
        }
        config.public_origin = public_origin.clone();
        config.codex_command = codex_command.clone();
        config.claude_command = claude_command.clone();
        config.ollama_url = ollama_url.clone();
        return if *desktop_managed {
            bibi_server::serve_with_shutdown(config, desktop_closed()).await
        } else {
            bibi_server::serve(config).await
        };
    }
    if args.command.is_none() {
        return open_desktop(&args).await;
    }
    if matches!(args.command, Some(Action::Auth)) {
        let config = ServiceConfig::new(args.data_dir.clone().unwrap_or_else(default_data_dir));
        println!("{}", config.prepare()?);
        return Ok(());
    }
    let client = Client::new(&args)?;
    let direct_guild = args.server.is_none();
    match args.command.unwrap() {
        Action::Host {
            name,
            url,
            project,
            workspace,
            guild,
            peer_token_file,
        } => {
            let token = std::fs::read_to_string(peer_token_file)
                .context("호스트 토큰 파일을 읽을 수 없습니다.")?;
            print(
                client
                    .command(Command::RegisterHost {
                        name,
                        url,
                        token,
                        project_key: project,
                        workspace,
                        guild_path: guild,
                    })
                    .await?,
            )?;
        }
        Action::Status => print(client.get("/api/snapshot").await?)?,
        Action::Runs { project } => {
            let mut data = client.get("/api/snapshot").await?;
            let runs = data["runs"].as_array_mut().context("잘못된 응답")?;
            if let Some(project) = project {
                runs.retain(|r| r["project_key"] == project);
            }
            print(json!(runs))?;
        }
        Action::Show { run_id } => print(client.get(&format!("/api/runs/{run_id}")).await?)?,
        Action::Projects => print(client.get("/api/snapshot").await?["projects"].clone())?,
        Action::Project {
            name,
            workspace,
            guild,
        } => print(
            client
                .command(Command::CreateProject {
                    name,
                    workspace: workspace.to_string_lossy().into(),
                    guild_path: guild.map(|p| p.to_string_lossy().into_owned()),
                    constraints: vec![],
                })
                .await?,
        )?,
        Action::Ask {
            question,
            project,
            from,
            provider: p,
            model,
            role,
            host,
            read_only,
            steer,
            fresh,
            submission_id,
        } => {
            let target = if let Some(id) = &from {
                Some(client.get(&format!("/api/runs/{id}")).await?)
            } else {
                None
            };
            let run = target.as_ref().map(|v| &v["run"]);
            let project_key = project
                .or_else(|| {
                    run.and_then(|r| r["project_key"].as_str())
                        .map(String::from)
                })
                .context("--project 또는 --from이 필요합니다.")?;
            let work_id = run.and_then(|r| r["work_id"].as_str()).map(String::from);
            let revision = if let Some(w) = &work_id {
                client.get(&format!("/api/works/{w}")).await?["context_revision"].as_u64()
            } else {
                None
            };
            let continuing = run.is_some() && !fresh && !steer;
            let snapshot: Snapshot = serde_json::from_value(client.get("/api/snapshot").await?)?;
            let provider_id = p.or_else(|| run.and_then(|r|r["provider_id"].as_str()).map(String::from))
                .or_else(||snapshot.model_selection.as_ref().map(|s|s.provider_id.clone()))
                .context("--provider에 등록한 제공자 ID를 지정하세요. bibi status에서 확인할 수 있습니다.")?;
            let configured = snapshot
                .providers
                .iter()
                .find(|p| p.id == provider_id)
                .context("등록된 제공자 ID가 아닙니다.")?;
            let provider = configured.adapter.clone();
            let host = host.unwrap_or_else(|| configured.host_id.clone());
            let host_id = if steer || continuing {
                run.unwrap()["host_id"].as_str().unwrap_or(&host).into()
            } else {
                host
            };
            let model = if continuing {
                run.unwrap()["model"].as_str().unwrap_or("").into()
            } else if model.is_empty() {
                snapshot
                    .model_selection
                    .as_ref()
                    .filter(|s| s.provider_id == provider_id)
                    .map(|s| s.model.clone())
                    .unwrap_or_default()
            } else {
                model
            };
            let role = if continuing {
                run.unwrap()["role"].as_str().unwrap_or(&role).into()
            } else {
                role
            };
            let read_only = if continuing {
                run.unwrap()["read_only"].as_bool().unwrap_or(read_only)
            } else {
                read_only
            };
            let request = Submission {
                submission_id: submission_id.unwrap_or_else(|| id("submission")),
                project_key,
                work_id,
                title: None,
                question,
                provider,
                provider_id: Some(provider_id),
                model,
                host_id,
                role,
                mode: if steer {
                    SubmitMode::Steer
                } else if continuing {
                    SubmitMode::Continue
                } else {
                    SubmitMode::Fresh
                },
                target_run_id: from,
                expected_turn_id: run.and_then(|r| r["turn_id"].as_str()).map(String::from),
                expected_context_revision: revision,
                read_only,
            };
            print(client.command(Command::Submit { request }).await?)?;
        }
        Action::Inbox { run, response } => {
            let path = if let Some(response) = response {
                format!("/api/inbox/{response}")
            } else if let Some(run) = run {
                let r = client.get(&format!("/api/runs/{run}")).await?;
                format!(
                    "/api/inbox?conversation_id={}",
                    r["run"]["conversation_id"].as_str().unwrap_or("")
                )
            } else {
                "/api/inbox".into()
            };
            print(client.get(&path).await?)?;
        }
        Action::Wait { run_id, seconds } => {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds.min(60));
            loop {
                let detail = client.get(&format!("/api/runs/{run_id}")).await?;
                let state: RunState = serde_json::from_value(detail["run"]["state"].clone())?;
                if state.terminal()
                    || matches!(state, RunState::WaitingUser)
                    || tokio::time::Instant::now() >= deadline
                {
                    print(detail)?;
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        Action::Interrupt { run_id } => {
            print(client.command(Command::Interrupt { run_id }).await?)?
        }
        Action::Resolve {
            run_id,
            confirmed_stopped,
        } => print(
            client
                .command(Command::ResolveRun {
                    run_id,
                    confirmed_stopped,
                })
                .await?,
        )?,
        Action::Context { work_id, file } => {
            if let Some(file) = file {
                let update = serde_json::from_slice(&std::fs::read(file)?)?;
                print(
                    client
                        .command(Command::UpdateContext { work_id, update })
                        .await?,
                )?;
            } else {
                print(client.get(&format!("/api/works/{work_id}")).await?)?;
            }
        }
        Action::Command { file } => print(
            client
                .command(serde_json::from_slice(&std::fs::read(file)?)?)
                .await?,
        )?,
        Action::Events { after } => {
            print(client.get(&format!("/api/events?after={after}")).await?)?
        }
        Action::Guild { project, args } => {
            if !direct_guild {
                bail!(
                    "guild CLI는 해당 실행 호스트에서 사용하세요. 원격 서버의 경로를 이 컴퓨터에서 실행하지 않습니다."
                );
            }
            let snapshot = client.get("/api/snapshot").await?;
            let projects = snapshot["projects"].as_array().context("잘못된 응답")?;
            let p = projects
                .iter()
                .find(|p| p["id"] == project)
                .context("프로젝트를 찾을 수 없습니다.")?;
            let path = p["guild_path"]
                .as_str()
                .context("연결된 길드가 없습니다.")?;
            let status = std::process::Command::new("openguild")
                .arg("--guild")
                .arg(path)
                .args(args)
                .status()?;
            if !status.success() {
                bail!("openguild가 실패했습니다: {status}");
            }
        }
        Action::Server { .. } | Action::Auth => unreachable!(),
    }
    Ok(())
}
async fn desktop_closed() {
    let (closed, received) = tokio::sync::oneshot::channel();
    // A dedicated OS thread can be abandoned when Ctrl+C wins. Tokio's stdin
    // reader uses its blocking pool, which would hold runtime shutdown open.
    std::thread::spawn(move || {
        use std::io::Read;
        let mut input = std::io::stdin().lock();
        let mut byte = [0];
        while matches!(input.read(&mut byte), Ok(n) if n > 0) {}
        let _ = closed.send(());
    });
    let _ = received.await;
}

async fn open_desktop(args: &Args) -> Result<()> {
    let executable = std::env::current_exe()?;
    let name = if cfg!(windows) {
        "bibi-desktop.exe"
    } else {
        "bibi-desktop"
    };
    let desktop = executable.parent().unwrap().join(name);
    if !desktop.exists() {
        bail!(
            "데스크톱 실행 파일이 없습니다: {}. 서버 실행은 bibi server를 사용하세요.",
            desktop.display()
        );
    }
    let mut command = std::process::Command::new(desktop);
    if let Some(dir) = &args.data_dir {
        command.env("BIBI_DATA_DIR", dir);
    }
    if let Some(server) = &args.server {
        command.env("BIBI_SERVER", server);
    }
    if let Some(path) = &args.token_file {
        command.env("BIBI_TOKEN_FILE", path);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}
