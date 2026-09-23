use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub workspace: String,
    pub guild_path: Option<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Evidence {
    pub text: String,
    pub source: String,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Work {
    pub id: String,
    pub project_key: String,
    pub conversation_id: String,
    pub title: String,
    pub goal: String,
    pub context_revision: u64,
    pub constraints: Vec<String>,
    pub decisions: Vec<Evidence>,
    pub performed_actions: Vec<Evidence>,
    pub open_questions: Vec<Evidence>,
    pub references: Vec<Evidence>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Mock,
    Codex,
    Ollama,
    Claude,
    OpenAi,
    Command,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Running,
    WaitingUser,
    WaitingExpert,
    Completed,
    Failed,
    Interrupted,
    Uncertain,
    Disconnected,
}
impl RunState {
    pub fn active(&self) -> bool {
        matches!(
            self,
            Self::Running | Self::WaitingUser | Self::WaitingExpert | Self::Disconnected
        )
    }
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Interrupted | Self::Uncertain
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Managed,
    External,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Capability {
    pub supported: bool,
    pub reason: Option<String>,
}
impl Capability {
    pub fn yes() -> Self {
        Self {
            supported: true,
            reason: None,
        }
    }
    pub fn no(reason: &str) -> Self {
        Self {
            supported: false,
            reason: Some(reason.into()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub open_history: Capability,
    pub stream_output: Capability,
    pub send_to_active: Capability,
    pub respond_to_input: Capability,
    pub start_fresh: Capability,
    #[serde(default)]
    pub continue_session: Capability,
    pub interrupt: Capability,
    pub read_quota: Capability,
}
impl Capabilities {
    pub fn managed(provider: &Provider) -> Self {
        let available = Capability::yes();
        Self {
            open_history: Capability::yes(),
            stream_output: available.clone(),
            send_to_active: if matches!(provider, Provider::Codex | Provider::Mock) {
                Capability::yes()
            } else {
                Capability::no("현재 응답이 끝나면 같은 세션에서 대화를 이어갈 수 있습니다.")
            },
            respond_to_input: if matches!(provider, Provider::Codex | Provider::Claude) {
                Capability::yes()
            } else {
                Capability::no("이 연결에는 별도 승인 요청이 없습니다.")
            },
            start_fresh: available.clone(),
            continue_session: available.clone(),
            interrupt: available,
            read_quota: if matches!(provider, Provider::Codex | Provider::Claude) {
                Capability::yes()
            } else {
                Capability::no("서비스 잔여 한도를 제공하지 않습니다.")
            },
        }
    }
    pub fn external(provider: &Provider) -> Self {
        let mut c = Self::managed(provider);
        c.stream_output =
            Capability::no("외부 실행을 소유하지 않습니다. 저장된 기록을 새로 조회할 수 있습니다.");
        c.send_to_active = Capability::no("외부 앱의 실행 중인 턴에 연결되지 않았습니다.");
        c.respond_to_input = c.send_to_active.clone();
        c.interrupt = c.send_to_active.clone();
        c.continue_session =
            Capability::no("외부 세션은 소유권을 확인할 수 없습니다. 별도 세션으로 시작하세요.");
        c
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageStats {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextPacket {
    pub schema_version: u8,
    pub project_key: String,
    pub work_id: String,
    pub conversation_id: String,
    pub request_id: String,
    pub parent_request_id: Option<String>,
    pub reply_to_response_id: Option<String>,
    pub context_revision: u64,
    pub role: String,
    pub question: String,
    pub goal: String,
    pub constraints: Vec<String>,
    pub decisions: Vec<Evidence>,
    pub performed_actions: Vec<Evidence>,
    pub open_questions: Vec<Evidence>,
    pub references: Vec<Evidence>,
    pub source_runs: Vec<String>,
    pub previous_answer_excerpt: Option<String>,
    pub excerpt_truncated: bool,
    pub workspace: String,
    pub reply_to: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Activity {
    pub revision: u64,
    pub phase: String,
    pub summary: String,
    pub wait_reason: Option<String>,
    pub next_action: String,
    pub references: Vec<Evidence>,
    pub reported_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub continued_from: Option<String>,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub agent_kind: String,
    pub project_key: String,
    pub work_id: String,
    pub conversation_id: String,
    pub request_id: String,
    pub parent_run_id: Option<String>,
    pub context_revision: u64,
    pub role: String,
    pub title: String,
    pub provider: Provider,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub model: String,
    pub host_id: String,
    pub state: RunState,
    pub phase: String,
    pub wait_reason: Option<String>,
    pub observation_source: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub observed_at: i64,
    pub session_key: Option<String>,
    pub turn_id: Option<String>,
    pub origin: Origin,
    pub workspace: String,
    pub read_only: bool,
    pub capabilities: Capabilities,
    pub context: ContextPacket,
    pub stats: UsageStats,
    #[serde(default, skip_serializing_if = "RuntimeMetadata::is_empty")]
    pub runtime: RuntimeMetadata,
    #[serde(default)]
    pub activity: Option<Activity>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub run_id: String,
    pub role: String,
    pub text: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transmission {
    pub id: String,
    pub from_run_id: Option<String>,
    pub to_run_id: String,
    pub request_id: String,
    pub response_id: Option<String>,
    pub kind: String,
    pub sent_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboxEntry {
    pub response_id: String,
    pub request_id: String,
    pub from_run_id: String,
    pub requester_run_id: Option<String>,
    pub to_conversation_id: String,
    pub context_revision: u64,
    pub result: String,
    #[serde(default)]
    pub late: bool,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub seq: i64,
    pub id: String,
    pub kind: String,
    pub created_at: i64,
    pub data: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubmitMode {
    Continue,
    Fresh,
    Steer,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Submission {
    pub submission_id: String,
    pub project_key: String,
    pub work_id: Option<String>,
    pub title: Option<String>,
    pub question: String,
    pub provider: Provider,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: String,
    pub host_id: String,
    pub role: String,
    pub mode: SubmitMode,
    pub target_run_id: Option<String>,
    pub expected_turn_id: Option<String>,
    pub expected_context_revision: Option<u64>,
    #[serde(default)]
    pub read_only: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub submission_id: String,
    pub run_id: String,
    pub request_id: String,
    pub work_id: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingInput {
    pub id: String,
    pub run_id: String,
    pub expected_turn_id: String,
    pub text: String,
    pub state: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub run_id: String,
    pub native_id: Value,
    pub kind: String,
    pub title: String,
    pub detail: Value,
    pub state: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub kind: String,
    pub connected: bool,
    pub observed_at: i64,
    pub providers: Vec<Provider>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub label: String,
    pub remaining_percent: Option<f64>,
    pub duration_minutes: Option<i64>,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quota {
    pub id: String,
    pub provider: Provider,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub account: String,
    pub host_id: String,
    pub model: Option<String>,
    pub status: String,
    pub windows: Vec<QuotaWindow>,
    pub observed_at: Option<i64>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub server_id: String,
    pub version: String,
    pub last_seq: i64,
    pub projects: Vec<Project>,
    pub works: Vec<Work>,
    pub runs: Vec<Run>,
    #[serde(default)]
    pub removed_sessions: Vec<Run>,
    pub transmissions: Vec<Transmission>,
    pub inbox: Vec<InboxEntry>,
    pub hosts: Vec<Host>,
    pub quotas: Vec<Quota>,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub model_history: Vec<ModelHistory>,
    #[serde(default)]
    pub model_selection: Option<ModelSelection>,
    pub approvals: Vec<Approval>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunDetail {
    pub run: Run,
    #[serde(default)]
    pub conversation: Vec<Message>,
    #[serde(default)]
    pub children: Vec<AgentDetail>,
    pub messages: Vec<Message>,
    pub inbox: Vec<InboxEntry>,
    pub approvals: Vec<Approval>,
    #[serde(default)]
    pub inputs: Vec<PendingInput>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextUpdate {
    pub expected_revision: u64,
    pub goal: String,
    pub constraints: Vec<String>,
    pub decisions: Vec<Evidence>,
    pub performed_actions: Vec<Evidence>,
    pub open_questions: Vec<Evidence>,
    pub references: Vec<Evidence>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardJob {
    pub run: Run,
    pub project: Project,
    pub work: Work,
}

impl Run {
    pub fn session_id(&self) -> &str {
        if self.session_id.is_empty() {
            &self.id
        } else {
            &self.session_id
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentDetail {
    pub run: Run,
    pub messages: Vec<Message>,
}

pub struct SubagentUpdate {
    pub native_id: String,
    pub event_id: String,
    pub title: String,
    pub state: Option<RunState>,
    pub prompt: Option<String>,
    pub text: Option<String>,
    pub stats: Option<UsageStats>,
    pub started: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommand {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub argument_hint: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMetadata {
    #[serde(default)]
    pub provider_name: Option<String>,
    #[serde(default)]
    pub commands: Vec<SlashCommand>,
    #[serde(default)]
    pub session_file: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    #[serde(default = "local_host")]
    pub host_id: String,
    #[serde(default)]
    pub remote_id: Option<String>,
    pub name: String,
    pub adapter: Provider,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub api_key_set: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelSelection {
    pub provider_id: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelHistory {
    pub provider_id: String,
    pub model: String,
    pub uses: u64,
    pub last_used: i64,
}

fn local_host() -> String {
    "local".into()
}
pub fn remote_provider_id(host_id: &str, id: &str) -> String {
    format!("remote:{host_id}:{id}")
}

impl RuntimeMetadata {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}
