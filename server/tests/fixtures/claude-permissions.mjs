import assert from 'node:assert/strict';
import fs from 'node:fs';
import readline from 'node:readline';

// Real Claude 2.1.296 emits status.permissionMode=plan after EnterPlanMode.
// That state persists into the next user message until set_permission_mode.
const args = process.argv.slice(2);
const resume = args.find(a => a.startsWith('--resume='))?.split('=')[1];
const session = resume || args.find(a => a.startsWith('--session-id='))?.split('=')[1];
let permission = args[args.indexOf('--permission-mode') + 1];
const record = session + '.permissions.json';
const history = resume ? JSON.parse(fs.readFileSync(record, 'utf8')) : [];
const input = readline.createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const send = value => process.stdout.write(JSON.stringify(value) + '\n');
const receive = async () => { const row = await input.next(); assert(!row.done); return JSON.parse(row.value); };
const reply = (v, response = {}) => send({type:'control_response', response:{subtype:'success', request_id:v.request_id, response}});
async function approval(reason) {
  send({type:'control_request', request_id:'permission-' + history.length, request:{subtype:'can_use_tool', tool_name:'Write', input:{file_path:'fixture.ts', content:'const value = 1;'}, decision_reason:reason, tool_use_id:'write-' + history.length}});
  return (await receive()).response.response.behavior === 'allow';
}
for (;;) {
  const row = await input.next(); if (row.done) break;
  const v = JSON.parse(row.value);
  if (v.type === 'control_request') {
    if (v.request.subtype === 'set_permission_mode') {
      if(v.request.mode==='bypassPermissions'&&fs.existsSync('disable-bypass')){send({type:'control_response',response:{subtype:'error',request_id:v.request_id,error_code:'bypass_disabled',error:'Cannot set permission mode to bypassPermissions because it is disabled by settings or configuration'}});continue;}
      if (fs.existsSync('reject-permission')) {
        send({type:'control_response', response:{subtype:'error', request_id:v.request_id, error:'fixture mode rejected'}});
        continue;
      }
      permission = v.request.mode;
    }
    reply(v); continue;
  }
  if (v.type !== 'user') continue;
  const question = v.message.content;
  history.push(question); fs.writeFileSync(record, JSON.stringify(history));
  send({type:'system', subtype:'init', session_id:session, permissionMode:permission});
  if (question.includes('PLAN_WITHIN_TURN')) {
    permission = 'plan';
    send({type:'system', subtype:'status', status:null, session_id:session, permissionMode:permission});
  }
  // A subagent's metadata must not replace the parent's current mode.
  if (question.includes('CHILD_MODE')) send({type:'system', subtype:'status', status:null, session_id:session, parent_tool_use_id:'child', permissionMode:'bypassPermissions'});
  const explicitAsk = question.includes('EXPLICIT_ASK');
  const written = explicitAsk || !['acceptEdits', 'bypassPermissions'].includes(permission)
    ? await approval(explicitAsk ? 'Explicit ask rule requires approval' : 'Plan mode requires approval') : true;
  if (written) fs.writeFileSync('edit-' + history.length + '.ts', 'export const value = 1;\n');
  if (question.includes('EDIT_THEN_PLAN')) {
    permission = 'plan';
    send({type:'system', subtype:'status', status:null, session_id:session, permissionMode:permission});
  }
  send({type:'result', session_id:session, is_error:false, result:JSON.stringify({written, permission, pid:process.pid, resumed:!!resume}), usage:{}});
}
