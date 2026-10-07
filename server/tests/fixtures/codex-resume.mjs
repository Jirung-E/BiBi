import fs from 'node:fs';
import readline from 'node:readline';
import path from 'node:path';
import assert from 'node:assert/strict';
const [file] = process.argv.slice(2);
const write = value => process.stdout.write(JSON.stringify(value)+'\n');
let resumed = false;
for await (const raw of readline.createInterface({input:process.stdin})) {
 const r=JSON.parse(raw),data=JSON.parse(fs.readFileSync(file,'utf8'));
 fs.appendFileSync(file+'.calls',JSON.stringify(r)+'\n');
 if(r.id===undefined)continue;
 let result={};
 const thread={id:data.native,cwd:data.cwd,createdAt:1,updatedAt:data.turns.length+1,model:'fixture-model',status:{type:data.busy?'active':'notLoaded'},path:file,preview:'external fixture'};
 switch(r.method){
 case 'initialize':break;
 case 'thread/list':result={data:[thread],nextCursor:null};break;
 case 'thread/read':
  if(data.missing){write({id:r.id,error:{code:-32000,message:'original thread missing'}});continue;}
  assert.equal(r.params.threadId,data.native);result={thread:{...thread,id:data.wrong_id?'different-thread':data.native}};break;
 case 'thread/turns/list':result={data:[...data.turns].reverse(),nextCursor:null};break;
 case 'thread/resume':assert.equal(r.params.threadId,data.native);resumed=true;result={thread:{...thread,id:data.fork?'different-thread':data.native},model:'fixture-model'};break;
 case 'thread/start':throw new Error('External resume must never start a fresh thread');
 case 'turn/start':{
  assert(resumed);assert.equal(r.params.threadId,data.native);
  const tid='turn-'+(data.turns.length+1),text=JSON.stringify({turns:data.turns.length+1,first:data.turns[0].items[0].content[0].text,resumed,pid:process.pid});
  data.turns.push({id:tid,status:'completed',startedAt:2,items:[{type:'userMessage',id:tid+'-u',content:[{type:'text',text:r.params.input[0].text}]},{type:'agentMessage',id:tid+'-a',text,phase:'final_answer'}]});
  fs.writeFileSync(file,JSON.stringify(data));
  write({id:r.id,result:{turn:{id:tid,status:'inProgress'}}});
  write({method:'item/completed',params:{threadId:data.native,turnId:tid,item:{type:'agentMessage',id:tid+'-a',text,phase:'final_answer'}}});
  write({method:'turn/completed',params:{threadId:data.native,turn:{id:tid,status:'completed'}}});continue;
 }
 default:write({id:r.id,error:{code:-32601,message:'unsupported fixture command'}});continue;
 }
 write({id:r.id,result});
}
