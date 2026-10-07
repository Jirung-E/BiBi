import fs from 'node:fs';
import readline from 'node:readline';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const file=process.argv[2],send=value=>process.stdout.write(JSON.stringify(value)+'\n');
let resumed;
for await(const raw of readline.createInterface({input:process.stdin})){
 const r=JSON.parse(raw),data=JSON.parse(fs.readFileSync(file,'utf8'));
 fs.appendFileSync(file+'.calls',JSON.stringify(r)+'\n');
 if(r.id===undefined)continue;
 let result={},t=data.threads[r.params?.threadId];
 switch(r.method){
 case 'initialize':break;
 case 'thread/read':assert(t);result={thread:t};break;
 case 'thread/turns/list':assert(t);result={data:[...t.turns].reverse(),nextCursor:null};break;
 case 'thread/fork':{
  assert(t);assert(r.params.lastTurnId);assert.equal(r.params.ephemeral,false);
  if(data.unsupported){send({id:r.id,error:{code:-32601,message:'thread/fork unsupported'}});continue;}
  const end=t.turns.findIndex(t=>t.id===r.params.lastTurnId);assert(end>=0);
  const fork={...t,id:randomUUID(),forkedFromId:t.id,turns:t.turns.slice(0,data.ignoreCutoff?undefined:end+1)};
  data.threads[fork.id]=fork;fs.writeFileSync(file,JSON.stringify(data));result={thread:fork};break;
 }
 case 'thread/resume':assert(t);resumed=t.id;result={thread:t,model:'fixture-model'};break;
 case 'turn/start':{
  assert.equal(resumed,t.id);
  const text=JSON.stringify({native:t.id,history:t.turns,question:r.params.input});
  const turn={id:randomUUID(),status:'completed',items:[{id:randomUUID(),type:'agentMessage',text,phase:'final_answer'}]};
  t.turns.push(turn);fs.writeFileSync(file,JSON.stringify(data));
  send({id:r.id,result:{turn:{id:turn.id,status:'inProgress'}}});
  send({method:'item/completed',params:{threadId:t.id,turnId:turn.id,item:turn.items[0]}});
  send({method:'turn/completed',params:{threadId:t.id,turn}});continue;
 }
 default:throw Error('unexpected method '+r.method);
 }
 send({id:r.id,result});
}
