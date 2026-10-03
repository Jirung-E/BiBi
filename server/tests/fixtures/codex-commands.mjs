import fs from 'node:fs';import readline from 'node:readline';import path from 'node:path';
const [record]=process.argv.slice(2);const write=value=>process.stdout.write(JSON.stringify(value)+'\n');let turn=0;
const id='native-command-thread';
for await(const raw of readline.createInterface({input:process.stdin})){
 const r=JSON.parse(raw);fs.appendFileSync(record,JSON.stringify(r)+'\n');
 if(r.id===undefined)continue;
 let result={};
 switch(r.method){
 case 'initialize':break;
 case 'thread/start':case 'thread/resume': result={thread:{id,path:path.join(process.cwd(),'session.jsonl')},model:'fixture-model'};break;
 case 'skills/list':result={data:[{skills:[{name:'native-skill',path:path.join(process.cwd(),'SKILL.md'),description:'test skill',enabled:true}],errors:[]}]};break;
 case 'turn/start':case 'review/start':{const tid='turn-'+(++turn);result={turn:{id:tid,status:'inProgress'},reviewThreadId:id};write({id:r.id,result});write({method:'item/completed',params:{threadId:id,turnId:tid,item:{type:'agentMessage',id:'message-'+turn,text:'fixture result',phase:'final_answer'}}});write({method:'turn/completed',params:{threadId:id,turn:{id:tid,status:'completed'}}});continue;}
 case 'thread/compact/start':write({id:r.id,result:{}});write({method:'thread/compacted',params:{threadId:id}});continue;
 default:write({id:r.id,error:{code:-32601,message:'unsupported fixture command'}});continue;
 }
 write({id:r.id,result});
}
