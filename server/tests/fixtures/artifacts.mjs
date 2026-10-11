import assert from 'node:assert/strict';
import readline from 'node:readline';
const send=v=>process.stdout.write(JSON.stringify(v)+'\n');
const native='artifact-native';let replies=0,first;
const tool=()=>send({id:'publish',method:'item/tool/call',params:{threadId:native,turnId:'t1',callId:'publish-once',tool:'bibi_publish_file',arguments:{name:'result.csv',content:'name,count\ncedar,12\n'}}});
for await(const line of readline.createInterface({input:process.stdin})){
 const r=JSON.parse(line);
 if(r.id==='publish'){
  assert.equal(r.result.success,true);const result=JSON.parse(r.result.contentItems[0].text);
  if(replies++===0){first=result;tool();continue;}assert.deepEqual(result,first);
  send({method:'item/completed',params:{threadId:native,turnId:'t1',item:{id:'a1',type:'agentMessage',phase:'final_answer',text:'[CSV]('+result.url+')'}}});
  send({method:'turn/completed',params:{threadId:native,turn:{id:'t1',status:'completed'}}});continue;
 }
 if(r.id===undefined)continue;
 if(r.method==='thread/start'){assert(r.params.dynamicTools.some(t=>t.name==='bibi_publish_file'));send({id:r.id,result:{thread:{id:native},model:'fixture'}});}
 else if(r.method==='turn/start'){send({id:r.id,result:{turn:{id:'t1',status:'inProgress'}}});tool();}
 else send({id:r.id,result:{}});
}
