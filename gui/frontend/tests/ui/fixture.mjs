// Deterministic UI data only: no model process, credentials, user files, or real API.
const time = 1_790_100_000_000;
const longName = 'layout-regression-with-a-long-unbroken-provider-model-and-workspace-name';
const supported = {supported: true, reason: null};
const capabilities = Object.fromEntries(['open_history', 'stream_output', 'send_to_active', 'respond_to_input', 'start_fresh', 'continue_session', 'interrupt', 'read_quota'].map(key => [key, supported]));
const project = {id: 'layout-project', name: '레이아웃 검증 프로젝트 ' + longName, workspace: '/fixture/' + longName, guild_path: null, constraints: ['UI fixture only']};
const work = {id: 'layout-work', project_key: project.id, conversation_id: 'layout-conversation', title: '화면 배율과 긴 대화의 레이아웃 검증', goal: '긴 모델 이름과 코드·표가 있어도 모든 입력 컨트롤에 접근할 수 있어야 합니다.', context_revision: 1, constraints: ['실제 모델 호출 없음', longName], decisions: [], performed_actions: [], open_questions: [], references: [], created_at: time};
function run(id, title, parent = null, kind = 'session', workId = work.id) {
  return {id, session_id: id, continued_from: null, parent_session_id: parent, agent_kind: kind, project_key: project.id, work_id: workId, conversation_id: work.conversation_id, request_id: id + '-request', parent_run_id: parent, context_revision: 1, role: kind === 'session' ? 'coordinator' : 'reviewer', title, provider: 'mock', provider_id: 'layout-provider', runtime: {provider_name: 'UI 모의 제공자', commands: [{name: 'help', description: '도움말', argument_hint: ''}], session_file: '/fixture/' + longName + '/session.jsonl'}, model: 'fixture-model-' + longName, host_id: 'local', state: 'completed', phase: 'completed', wait_reason: null, observation_source: 'fixture', created_at: time, updated_at: time, observed_at: time, session_key: id, turn_id: id + '-turn', origin: 'managed', workspace: project.workspace, read_only: true, capabilities, context: {question: '레이아웃 확인', goal: work.goal, constraints: work.constraints, decisions: [], performed_actions: [], open_questions: [], references: [], source_runs: [], previous_answer_excerpt: null, excerpt_truncated: false, reply_to: work.conversation_id}, stats: {input_tokens: null, cached_input_tokens: null, output_tokens: null, duration_ms: null}, activity: null, error: null};
}
const runs = [
  run('layout-parent', '부모 세션 ' + longName),
  run('layout-child', '작은 서브에이전트', 'layout-parent', 'subagent'),
  run('layout-expert', '전문가 세션', 'layout-parent', 'expert'),
  run('layout-sibling', '다른 서브에이전트', 'layout-parent', 'subagent'),
  run('layout-other', '다른 업무 세션', null, 'session', 'layout-other-work'),
];
const provider = {id: 'layout-provider', host_id: 'local', remote_id: null, name: 'UI 검증 ' + longName, adapter: 'mock', command: '', args: [], endpoint: '', models: [runs[0].model], api_key_set: false};
export const snapshot = {
  server_id: 'layout-fixture', version: 'fixture', last_seq: 0,
  projects: [project], works: [work, {...work, id: 'layout-other-work', title: '다른 업무'}], runs,
  transmissions: [{id: 'layout-transmission', from_run_id: runs[0].id, to_run_id: runs[1].id, request_id: 'layout-request', response_id: null, kind: 'query', sent_at: time}],
  inbox: [], hosts: [{id: 'local', name: '검증 호스트', platform: 'fixture', kind: 'local', connected: true, observed_at: time, providers: ['mock'], error: null}],
  quotas: [{id: 'layout-quota', provider: 'mock', provider_id: provider.id, account: provider.name, host_id: 'local', model: runs[0].model, status: 'unlimited', windows: [], observed_at: time, reason: 'UI fixture'}],
  approvals: [], providers: [provider],
  model_history: Array.from({length: 8}, (_, i) => ({provider_id: provider.id, model: i === 0 ? runs[0].model : 'recent-' + i + '-' + longName, uses: 8 - i, last_used: time - i})),
  model_selection: {provider_id: provider.id, model: runs[0].model}, removed_sessions: [],
};
const markdown = '# 응답 제목\n\n긴 대화에서도 입력창이 잘리지 않아야 합니다.\n\n- 첫 항목\n- 두 번째 항목\n\n```text\n' + longName.repeat(6) + '\n```\n\n| 모델 | 경로 | 상태 |\n|---|---|---|\n| ' + longName + ' | ' + longName.repeat(3) + ' | 완료 |';
export function detail(id) {
  const selected = runs.find(item => item.id === id);
  if (!selected) return null;
  const messages = Array.from({length: 24}, (_, i) => ({id: id + '-message-' + i, run_id: id, role: i % 2 ? 'assistant' : 'user', text: i % 2 ? markdown : '이전 대화를 이어서 확인합니다. ' + i, created_at: time + i * 1000}));
  return {run: selected, messages, conversation: messages, inbox: [], approvals: [], inputs: []};
}
