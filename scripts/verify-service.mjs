import {spawn} from 'node:child_process';
import {mkdtemp,readFile,rm,mkdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {setTimeout as sleep} from 'node:timers/promises';
const root=await mkdtemp(path.join(tmpdir(),'bibi-platform-'));
const workspace=path.join(root,'workspace');await mkdir(workspace);
const binary=process.argv[2]||path.resolve('target/debug/bibi'+(process.platform==='win32'?'.exe':''));
const child=spawn(binary,['--data-dir',root,'server','--bind','127.0.0.1:0'],{stdio:['ignore','ignore','pipe']});
let diagnostic='';child.stderr.on('data',data=>diagnostic+=data.toString());
let exited=false;child.on('exit',()=>exited=true);child.on('error',error=>{diagnostic+=error.message;exited=true;});
try{
 let info,token;
 for(let attempt=0;attempt<50;attempt++){
  if(exited)throw new Error(diagnostic);
  try{info=JSON.parse(await readFile(path.join(root,'service.json'),'utf8'));token=(await readFile(path.join(root,'access-token'),'utf8')).trim();break;}catch{await sleep(100);}
 }
 assert.ok(info,'service startup timed out');
 const request=async(route,body)=>{
  const response=await fetch(info.url+route,{method:body?'POST':'GET',headers:{Authorization:'Bearer '+token,...(body?{'Content-Type':'application/json'}:{})},body:body?JSON.stringify(body):undefined});
  const value=await response.json();assert.equal(response.status,200,JSON.stringify(value));return value;
 };
 assert.equal((await fetch(info.url+'/api/snapshot')).status,401);
 const html=await fetch(info.url+'/');assert.equal(html.status,200);assert.match(await html.text(),/BiBi|_app/);
 const project=await request('/api/command',{type:'create_project',name:'Platform smoke',workspace,constraints:['Mock only']});
 const submit={submission_id:'initial',project_key:project.id,question:'Platform smoke',provider:'mock',model:'mock',host_id:'local',role:'coordinator',mode:'fresh',read_only:true};
 const first=await request('/api/command',{type:'submit',request:submit});
 assert.equal((await request('/api/command',{type:'submit',request:submit})).run_id,first.run_id);
 let detail;
 for(let i=0;i<80;i++){detail=await request('/api/runs/'+first.run_id);if(detail.run.state==='completed')break;await sleep(100);}
 assert.equal(detail.run.state,'completed');assert.equal(detail.inbox.length,1);
 const next=await request('/api/command',{type:'submit',request:{...submit,submission_id:'followup',question:'Follow up',mode:'continue',target_run_id:first.run_id,expected_turn_id:detail.run.turn_id,expected_context_revision:1}});
 assert.notEqual(next.run_id,first.run_id);assert.equal(next.work_id,first.work_id);
 for(let i=0;i<80;i++){const follow=await request('/api/runs/'+next.run_id);if(follow.run.state==='completed'){assert.equal(follow.run.session_key,detail.run.session_key);assert.equal(follow.run.session_id,detail.run.session_id);assert.ok(follow.conversation.length>follow.messages.length);break;}assert.ok(i<79,'follow-up timeout');await sleep(100);}
 const branch=await request('/api/command',{type:'submit',request:{...submit,submission_id:'branch',question:'Separate task',target_run_id:next.run_id,expected_context_revision:1}});
 assert.notEqual((await request('/api/runs/'+branch.run_id)).run.session_id,detail.run.session_id);
 const snapshot=await request('/api/snapshot');assert.ok(!JSON.stringify(snapshot).includes(token));
 console.log(JSON.stringify({platform:process.platform,static_ui:true,authentication:true,durable_retry:true,continuous_followup:true,explicit_fresh_session:true,runs:snapshot.runs.length}));
}finally{
 if(!exited){child.kill('SIGINT');for(let i=0;i<70&&!exited;i++)await sleep(100);if(!exited)child.kill('SIGKILL');}
 for(let i=0;i<10&&!exited;i++)await sleep(100);
 await rm(root,{recursive:true,force:true,maxRetries:5,retryDelay:200});
}
