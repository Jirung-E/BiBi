mod assets;
mod auth;
pub mod config;
pub mod local;
pub mod peer;
pub mod providers;
pub mod runtime;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    middleware,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event as SseEvent, KeepAlive},
    },
    routing::{get, post},
};
use bibi_core::*;
use config::ServiceConfig;
use runtime::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::result::Result;
use std::{
    convert::Infallible,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub engine: Engine,
    pub config: ServiceConfig,
    pub token: Arc<String>,
    pub stopping: Arc<AtomicBool>,
}
pub struct ApiError {
    status: StatusCode,
    message: String,
}
impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}
impl From<bibi_core::Error> for ApiError {
    fn from(e: bibi_core::Error) -> Self {
        let status = match e {
            bibi_core::Error::Invalid(_) => StatusCode::BAD_REQUEST,
            bibi_core::Error::NotFound(_) => StatusCode::NOT_FOUND,
            bibi_core::Error::Conflict(_) => StatusCode::CONFLICT,
            bibi_core::Error::Unsupported(_) => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!("{e}");
        }
        Self::new(
            status,
            if status == StatusCode::INTERNAL_SERVER_ERROR {
                "저장소 처리 오류입니다.".into()
            } else {
                e.to_string()
            },
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error":self.message}))).into_response()
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    CreateProject {
        name: String,
        workspace: String,
        guild_path: Option<String>,
        #[serde(default)]
        constraints: Vec<String>,
    },
    Submit {
        request: Submission,
    },
    ListForkPoints {
        run_id: String,
    },
    ForkSession {
        request: ForkRequest,
    },
    CheckExternalResume {
        run_id: String,
    },
    ResumeExternal {
        run_id: String,
        expected_native_id: String,
        #[serde(default)]
        confirmed_stopped: bool,
    },
    PrepareHostProject {
        project: Project,
    },
    ListCommands {
        project_key: String,
        provider_id: String,
        run_id: Option<String>,
    },
    ListProjectExtensions {
        project_key: String,
        provider_id: String,
    },
    UpdateProjectExtension {
        project_key: String,
        provider_id: String,
        edit: providers::extensions::Edit,
    },
    CheckProjectMcp {
        project_key: String,
        provider_id: String,
        id: String,
    },
    RefreshProviders,
    SaveProvider {
        provider: ProviderConfig,
        #[serde(default)]
        api_key: Option<String>,
    },
    CheckProvider {
        provider: ProviderConfig,
        #[serde(default)]
        api_key: Option<String>,
    },
    DeleteProvider {
        provider_id: String,
    },
    SelectModel {
        selection: ModelSelection,
    },
    RenameSession {
        run_id: String,
        title: String,
    },
    SetSessionGroups {
        run_id: String,
        work_ids: Vec<String>,
        expected_work_ids: Vec<String>,
    },
    SetSessionHidden {
        run_id: String,
        hidden: bool,
    },
    CleanupDisconnectedSessions {
        project_key: String,
        run_ids: Vec<String>,
    },
    RegisterHost {
        name: String,
        url: String,
        token: String,
        project_key: String,
        workspace: String,
        guild_path: Option<String>,
    },
    Discover {
        project_key: String,
        provider: Provider,
        #[serde(default)]
        provider_id: Option<String>,
    },
    Report {
        run_id: String,
        activity: Activity,
    },
    UpdateContext {
        work_id: String,
        update: ContextUpdate,
    },
    Interrupt {
        run_id: String,
    },
    ResolveRun {
        run_id: String,
        confirmed_stopped: bool,
    },
    Respond {
        approval_id: String,
        value: Value,
    },
}
pub fn state(store: Store, config: ServiceConfig, token: String) -> AppState {
    AppState {
        engine: Engine::new(store.clone(), config.clone()),
        store,
        config,
        token: Arc::new(token),
        stopping: Arc::new(AtomicBool::new(false)),
    }
}
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/snapshot", get(snapshot))
        .route("/runs/{id}", get(detail))
        .route("/runs/{id}/status", get(run_status))
        .route("/sessions/search", get(search_sessions))
        .route("/works/{id}", get(work))
        .route("/inbox", get(inbox))
        .route("/inbox/{id}", get(inbox_entry))
        .route("/submissions/{id}", get(receipt))
        .route("/events", get(events))
        .route("/stream", get(stream))
        .route("/command", post(command))
        .route(
            "/import",
            post(import_file).layer(DefaultBodyLimit::max(16 * 1024 * 1024 + 65536)),
        )
        .route("/host/validate", post(peer::validate))
        .route("/host/capabilities", get(peer::capabilities))
        .route("/host/execute", post(peer::accept))
        .route("/host/runs/{id}", get(peer::detail))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::guard));
    Router::new()
        .nest("/api", api)
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route(
            "/health",
            get(|| async { Json(json!({"product":PRODUCT_NAME,"version":VERSION})) }),
        )
        .fallback(assets::serve)
        .layer(DefaultBodyLimit::max(512 * 1024))
        .layer(CompressionLayer::new())
        .with_state(state)
}
pub async fn serve(config: ServiceConfig) -> anyhow::Result<()> {
    serve_with_shutdown(config, std::future::pending()).await
}

pub async fn serve_with_shutdown(
    config: ServiceConfig,
    requested: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let token = config.prepare()?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config.data_dir.join("service.lock"))?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("이 데이터 경로의 BiBi 서비스가 이미 실행 중입니다."))?;
    let listener = TcpListener::bind(config.bind).await?;
    let store = Store::open(config.data_dir.join("bibi.sqlite3"))?;
    let app = state(store, config.clone(), token);
    app.engine.start().await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let endpoint_file = config.data_dir.join("service.json");
    std::fs::write(
        endpoint_file,
        serde_json::to_vec(&json!({"url":endpoint,"pid":std::process::id()}))?,
    )?;
    tracing::info!("BiBi {VERSION} · {endpoint}");
    let shutdown = app.clone();
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async move {
            tokio::select! { _ = requested => {}, _ = shutdown_signal() => {} }
            shutdown.stopping.store(true, Ordering::Relaxed);
            shutdown.engine.stop().await;
        })
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
    }
    #[cfg(not(unix))]
    if tokio::signal::ctrl_c().await.is_err() {
        // A desktop-managed Windows server may have no console. Its lifetime
        // pipe remains the shutdown source instead of treating that as Ctrl+C.
        std::future::pending::<()>().await;
    }
}
async fn snapshot(State(s): State<AppState>) -> Result<Json<Snapshot>, ApiError> {
    Ok(Json(s.store.snapshot()?))
}
async fn detail(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunDetail>, ApiError> {
    Ok(Json(s.store.detail(&id)?))
}
async fn run_status(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Run>, ApiError> {
    Ok(Json(s.store.run(&id)?))
}
#[derive(Deserialize)]
struct SearchQuery {
    project: String,
    #[serde(default)]
    q: String,
}
async fn search_sessions(
    State(s): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<SessionSearchHit>>, ApiError> {
    Ok(Json(s.store.search_sessions(&q.project, &q.q)?))
}
async fn work(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Work>, ApiError> {
    Ok(Json(s.store.work(&id)?))
}
#[derive(Deserialize)]
struct InboxQuery {
    conversation_id: Option<String>,
}
async fn inbox(
    State(s): State<AppState>,
    Query(q): Query<InboxQuery>,
) -> Result<Json<Vec<InboxEntry>>, ApiError> {
    Ok(Json(s.store.inbox(q.conversation_id.as_deref())?))
}
async fn inbox_entry(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<InboxEntry>, ApiError> {
    Ok(Json(s.store.inbox_entry(&id)?))
}
async fn receipt(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Receipt>, ApiError> {
    s.store
        .receipt(&id)?
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "접수 이력이 없습니다."))
}
#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: i64,
}
async fn events(
    State(s): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Vec<Event>>, ApiError> {
    Ok(Json(s.store.events(q.after, 500)?))
}
async fn stream(
    State(s): State<AppState>,
    Query(q): Query<EventsQuery>,
    headers: axum::http::HeaderMap,
) -> Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>> {
    let after = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(q.after);
    let event_stream = futures_util::stream::unfold(
        (s.store, after, Vec::<Event>::new(), s.stopping),
        |(store, mut cursor, mut pending, stopping)| async move {
            loop {
                if stopping.load(Ordering::Relaxed) {
                    return None;
                }
                if let Some(event) = pending.pop() {
                    cursor = event.seq;
                    let frame = SseEvent::default()
                        .id(cursor.to_string())
                        .event("update")
                        .json_data(&event)
                        .unwrap();
                    return Some((Ok(frame), (store, cursor, pending, stopping)));
                }
                match store.events(cursor, 500) {
                    Ok(events) if !events.is_empty() => {
                        pending = events;
                        pending.reverse();
                    }
                    Ok(_) => tokio::time::sleep(Duration::from_millis(250)).await,
                    Err(error) => {
                        tracing::error!("event stream: {error}");
                        return None;
                    }
                }
            }
        },
    );
    Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("heartbeat"),
    )
}
async fn import_file(
    State(s): State<AppState>,
    Json(upload): Json<providers::imports::Upload>,
) -> Result<Json<Value>, ApiError> {
    tokio::task::spawn_blocking(move || providers::imports::upload(&s.engine, upload))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))
}
async fn command(
    State(s): State<AppState>,
    Json(cmd): Json<Command>,
) -> Result<Json<Value>, ApiError> {
    execute(&s, cmd).await.map(Json)
}
pub async fn execute(s: &AppState, cmd: Command) -> Result<Value, ApiError> {
    let result = match cmd {
        Command::CreateProject {
            name,
            workspace,
            guild_path,
            constraints,
        } => {
            let path = PathBuf::from(workspace).canonicalize().map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "호스트에 존재하는 작업 경로가 필요합니다.",
                )
            })?;
            if !path.is_dir() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "작업 경로는 폴더여야 합니다.",
                ));
            }
            serde_json::to_value(s.store.add_project(Project {
                id: id("project"),
                name,
                workspace: path.to_string_lossy().into_owned(),
                guild_path,
                constraints,
            })?)
        }
        Command::Submit { request } => {
            if request.provider != Provider::Mock && request.provider_id.is_none() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "사용할 제공자를 먼저 등록하고 선택하세요.",
                ));
            }
            serde_json::to_value(s.store.submit(request)?)
        }
        Command::ListForkPoints { run_id } => Ok(providers::forks::points(&s.engine, &run_id)
            .await
            .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?),
        Command::ForkSession { request } => serde_json::to_value(
            providers::forks::fork(&s.engine, &request)
                .await
                .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?,
        ),
        Command::CheckExternalResume { run_id } => {
            Ok(providers::external_resume::check(&s.engine, &run_id)
                .await
                .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?)
        }
        Command::ResumeExternal {
            run_id,
            expected_native_id,
            confirmed_stopped,
        } => serde_json::to_value(
            providers::external_resume::adopt(
                &s.engine,
                &run_id,
                &expected_native_id,
                confirmed_stopped,
            )
            .await
            .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?,
        ),
        Command::SaveProvider { provider, api_key } => {
            if !provider.endpoint.trim().is_empty() {
                providers::check::endpoint(&provider.endpoint)
                    .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            let saved = s.store.save_provider(provider, api_key)?;
            s.engine
                .discard_provider(&saved.id)
                .await
                .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?;
            serde_json::to_value(saved)
        }
        Command::CheckProvider { provider, api_key } => {
            serde_json::to_value(providers::check::check(&s.engine, &provider, api_key).await)
        }
        Command::DeleteProvider { provider_id } => {
            s.store.delete_provider(&provider_id)?;
            s.engine
                .discard_provider(&provider_id)
                .await
                .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?;
            Ok(json!({"deleted":true}))
        }
        Command::SelectModel { selection } => {
            s.store.select_model(selection)?;
            Ok(json!({"saved":true}))
        }
        Command::RenameSession { run_id, title } => {
            s.store.rename_session(&run_id, &title)?;
            Ok(json!({"saved":true}))
        }
        Command::SetSessionGroups {
            run_id,
            work_ids,
            expected_work_ids,
        } => serde_json::to_value(s.store.set_session_groups(
            &run_id,
            work_ids,
            expected_work_ids,
        )?),
        Command::SetSessionHidden { run_id, hidden } => {
            if hidden {
                providers::refresh_claude_observations(&s.engine)
                    .await
                    .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?;
            }
            s.store.set_session_hidden(&run_id, hidden)?;
            if hidden {
                s.engine
                    .discard_session(s.store.run(&run_id)?.session_id())
                    .await;
            }
            Ok(json!({"saved":true}))
        }
        Command::CleanupDisconnectedSessions {
            project_key,
            run_ids,
        } => {
            providers::refresh_claude_observations(&s.engine)
                .await
                .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?;
            // This only changes local visibility, including for unreachable remote hosts.
            // Do not discard a connection that might have recovered since the transaction.
            serde_json::to_value(
                s.store
                    .cleanup_disconnected_sessions(&project_key, &run_ids)?,
            )
        }
        Command::PrepareHostProject { mut project } => {
            project.workspace = peer::directory(&project.workspace)?;
            project.guild_path = project
                .guild_path
                .as_deref()
                .map(peer::directory)
                .transpose()?;
            serde_json::to_value(s.store.ensure_project(project)?)
        }
        Command::ListCommands {
            project_key,
            provider_id,
            run_id,
        } => Ok(providers::commands::catalog(
            &s.engine,
            &project_key,
            &provider_id,
            run_id.as_deref(),
        )
        .await
        .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?),
        Command::ListProjectExtensions {
            project_key,
            provider_id,
        } => Ok(
            providers::extensions::list(&s.engine, &project_key, &provider_id)
                .await
                .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?,
        ),
        Command::UpdateProjectExtension {
            project_key,
            provider_id,
            edit,
        } => Ok(
            providers::extensions::update(&s.engine, &project_key, &provider_id, edit)
                .await
                .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?,
        ),
        Command::CheckProjectMcp {
            project_key,
            provider_id,
            id,
        } => Ok(
            providers::extensions::check_mcp(&s.engine, &project_key, &provider_id, &id)
                .await
                .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?,
        ),
        Command::RefreshProviders => Ok(providers::refresh(&s.engine).await),
        Command::RegisterHost {
            name,
            url,
            token,
            project_key,
            workspace,
            guild_path,
        } => Ok(serde_json::to_value(
            peer::register(
                &s.engine,
                name,
                url,
                token,
                project_key,
                workspace,
                guild_path,
            )
            .await
            .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?,
        )
        .unwrap()),
        Command::Discover {
            project_key,
            provider,
            provider_id,
        } => {
            let provider_id = provider_id.ok_or_else(|| {
                ApiError::new(StatusCode::BAD_REQUEST, "조회할 제공자를 선택하세요.")
            })?;
            let engine = s
                .engine
                .configured(&provider_id)
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
            if engine
                .provider
                .as_ref()
                .is_none_or(|p| p.adapter != provider)
            {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "제공자 연결 방식이 변경되었습니다.",
                ));
            }
            if engine
                .provider
                .as_ref()
                .is_none_or(|p| p.host_id != "local")
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "이 호스트의 제공자를 선택하세요.",
                ));
            }
            Ok(match provider {
                Provider::Codex => providers::codex::discover(&engine, &project_key).await,
                Provider::Claude => tokio::task::spawn_blocking(move || {
                    providers::claude_history::discover(&engine, &project_key)
                })
                .await
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
                _ => {
                    return Err(ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "이 제공자는 세션 목록을 제공하지 않습니다. 대화 JSON 파일을 가져오세요.",
                    ));
                }
            }
            .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?)
        }
        Command::Report { run_id, activity } => {
            serde_json::to_value(s.store.report(&run_id, activity)?)
        }
        Command::UpdateContext { work_id, update } => {
            serde_json::to_value(s.store.update_context(&work_id, update)?)
        }
        Command::Interrupt { run_id } => serde_json::to_value(
            s.engine
                .interrupt(&run_id)
                .await
                .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?,
        ),
        Command::ResolveRun {
            run_id,
            confirmed_stopped,
        } => {
            if !confirmed_stopped {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "이전 프로세스의 종료와 변경 확인이 필요합니다.",
                ));
            }
            serde_json::to_value(s.store.resolve_uncertain(&run_id)?)
        }
        Command::Respond { approval_id, value } => {
            s.engine
                .respond(&approval_id, value)
                .await
                .map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?;
            Ok(json!({"accepted":true}))
        }
    };
    result.map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "응답을 생성하지 못했습니다.",
        )
    })
}
