import assert from 'node:assert/strict';
import fs from 'node:fs';
import readline from 'node:readline';
import { setTimeout as delay } from 'node:timers/promises';

const args = process.argv.slice(2);
const send = value => process.stdout.write(JSON.stringify(value) + '\n');
if (args.join(' ') === 'auth status --json') {
  send({ loggedIn: true, subscriptionType: 'test' });
  process.exit(0);
}
assert(args.includes('--input-format') && args.includes('--permission-prompt-tool'));
assert(!args.includes('--dangerously-skip-permissions') && !args.includes('--continue'));
const resume = args.find(a => a.startsWith('--resume='))?.split('=')[1];
const session = resume || args.find(a => a.startsWith('--session-id='))?.split('=')[1];
assert.match(session, /^[\da-f]{8}-(?:[\da-f]{4}-){3}[\da-f]{12}$/i);
let model = args.includes('--model') ? args[args.indexOf('--model') + 1] : 'fixture-claude';
let changes = 0;
let permission_mode = args[args.indexOf('--permission-mode') + 1];
let permission_changes = 0;
const validatePermission = mode => {
  assert(['default', 'acceptEdits', 'bypassPermissions'].includes(mode));
  if (mode === 'bypassPermissions') assert(args.includes('--allow-dangerously-skip-permissions'));
};
validatePermission(permission_mode);
const path = session + '.fixture.json';
const history = resume ? JSON.parse(fs.readFileSync(path, 'utf8')) : [];
const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity })[Symbol.asyncIterator]();
const receive = async () => {
  const line = await input.next();
  assert(!line.done, 'unexpected end of input');
  return JSON.parse(line.value);
};
const first = await receive();
assert.equal(first.request.subtype, 'initialize');
send({ type: 'control_request', request_id: 'mcp-init', request: { subtype: 'mcp_message', server_name: 'bibi', message: { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2024-11-05' } } } });
assert.equal((await receive()).response.response.mcp_response.result.serverInfo.name, 'bibi');
send({ type: 'control_response', response: { subtype: 'success', request_id: first.request_id, response: { commands: [{ name: 'compact', description: 'fixture compact' }, { name: 'model' }] } } });
while (true) {
  const line = await input.next();
  if (line.done) break;
  const value = JSON.parse(line.value);
  if (value.type === 'control_request' && value.request.subtype === 'set_model') {
    const requested = value.request.model;
    changes++;
    if (requested === 'reject-model') {
      send({ type: 'control_response', response: { subtype: 'error', request_id: value.request_id, error: 'fixture model rejected' } });
    } else {
      model = requested || 'fixture-claude';
      send({ type: 'control_response', response: { subtype: 'success', request_id: value.request_id, response: {} } });
    }
    continue;
  }
  if (value.type === 'control_request' && value.request.subtype === 'set_permission_mode') {
    const requested = value.request.mode;
    validatePermission(requested);
    if (fs.existsSync('reject-permission')) {
      send({ type: 'control_response', response: { subtype: 'error', request_id: value.request_id, error: 'fixture permission rejected' } });
    } else {
      permission_mode = requested;
      permission_changes++;
      send({ type: 'control_response', response: { subtype: 'success', request_id: value.request_id, response: {} } });
    }
    continue;
  }
  if (value.type !== 'user') continue;
  const question = value.message.content;
  history.push(question);
  fs.writeFileSync(path, JSON.stringify(history));
  send({ type: 'system', subtype: 'init', session_id: session, model, slash_commands: ['compact', 'model'] });
  if (question.includes('INTERRUPT_FIXTURE')) {
    assert.equal((await receive()).request.subtype, 'interrupt');
    send({ type: 'result', session_id: session, is_error: false, result: 'interrupted', usage: {} });
    continue;
  }
  if (question.includes('MESSAGE_FIXTURE')) {
    const tool = 'message-' + history.length;
    const task = 'task-' + history.length;
    send({ type: 'assistant', uuid: tool, session_id: session, message: { id: tool, content: [{ type: 'tool_use', id: tool, name: 'SendMessage', input: { to: 'native-expert', message: 'followup-' + history.length, summary: 'Continue existing expert' } }] } });
    send({ type: 'user', uuid: tool + '-receipt', session_id: session, tool_use_result: { success: true, resumedAgentId: 'native-expert' }, message: { content: [{ type: 'tool_result', tool_use_id: tool, content: 'Message queued' }] } });
    send({ type: 'result', session_id: session, is_error: false, result: 'Message sent; waiting for expert', usage: {} });
    await delay(100);
    send({ type: 'system', subtype: 'task_started', session_id: session, task_id: task, tool_use_id: tool, task_type: 'local_agent', description: '검토 전문가' });
    send({ type: 'control_request', request_id: tool + '-permission', request: { subtype: 'can_use_tool', tool_name: 'Bash', input: { command: 'fixture-only' }, tool_use_id: tool + '-bash' } });
    assert.equal((await receive()).response.response.behavior, 'allow');
    send({ type: 'assistant', uuid: tool + '-answer', session_id: session, parent_tool_use_id: tool, message: { id: tool + '-answer', content: [{ type: 'text', text: 'continued expert answer ' + history.length }] } });
    send({ type: 'system', subtype: 'task_notification', session_id: session, task_id: task, tool_use_id: tool, status: 'completed', summary: 'expert finished ' + history.length });
    continue;
  }
  if (history.length === 1) {
    send({ type: 'assistant', uuid: 'main-agent', session_id: session, parent_tool_use_id: null, message: { id: 'agent-spawn', content: [{ type: 'tool_use', id: 'agent-1', name: 'Agent', input: { description: '검토 전문가', prompt: 'fixture 자료만 검토' } }] } });
    send({ type: 'assistant', uuid: 'child', session_id: session, parent_tool_use_id: 'agent-1', message: { id: 'child-text', model: 'fixture-child-model', content: [{ type: 'text', text: '서브에이전트의 별도 답변' }] } });
    send({ type: 'user', uuid: 'agent-result', session_id: session, tool_use_result: { agentId: 'native-expert' }, message: { content: [{ type: 'tool_result', tool_use_id: 'agent-1', content: '검토 완료' }] } });
    send({ type: 'control_request', request_id: 'permission', request: { subtype: 'can_use_tool', tool_name: 'Bash', input: { command: 'fixture-only' }, tool_use_id: 'bash-1' } });
    const permission = (await receive()).response.response;
    assert.equal(permission.behavior, 'allow');
    assert.deepEqual(permission.updatedInput, { command: 'fixture-only' });
    send({ type: 'control_request', request_id: 'mcp-call', request: { subtype: 'mcp_message', server_name: 'bibi', message: { jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'bibi_list_files', arguments: { path: '.' } } } } });
    assert(!(await receive()).response.response.mcp_response.result.isError);
  }
  const text = JSON.stringify({ turns: history.length, model, model_changes: changes, permission_mode, permission_changes, pid: process.pid, resumed: !!resume, first: history[0], last: question });
  send({ type: 'stream_event', session_id: session, event: { type: 'message_start', message: { id: 'answer-' + history.length } } });
  send({ type: 'stream_event', session_id: session, event: { type: 'content_block_delta', delta: { type: 'text_delta', text } } });
  send({ type: 'assistant', session_id: session, message: { id: 'answer-' + history.length, content: [{ type: 'text', text }] } });
  send({ type: 'rate_limit_event', session_id: session, rate_limit_info: { status: 'allowed', rateLimitType: 'five_hour', utilization: .46, resetsAt: 1800000000 } });
  send({ type: 'result', session_id: session, is_error: false, result: text, duration_ms: 12, usage: { input_tokens: 10, cache_read_input_tokens: 30, output_tokens: 5 } });
}
