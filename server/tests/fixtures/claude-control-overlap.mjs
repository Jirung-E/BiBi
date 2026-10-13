import assert from 'node:assert/strict';import fs from 'node:fs';import readline from 'node:readline';
const args=process.argv.slice(2),session=args.find(a=>a.startsWith('--session-id=')).split('=')[1];const lines=readline.createInterface({input:process.stdin})[Symbol.asyncIterator]();const send=v=>process.stdout.write(JSON.stringify(v)+'\n');let turns=0,waiting=false;
for(;;){const row=await lines.next();if(row.done)break;const v=JSON.parse(row.value);
 if(v.type==='control_request'){
  if(v.request.subtype==='set_permission_mode'&&turns===1){waiting=true;send({type:'control_request',request_id:'late-write',request:{subtype:'can_use_tool',tool_name:'Write',input:{file_path:'late.txt',content:'fixture'},decision_reason_type:'rule'}});}
  send({type:'control_response',response:{subtype:'success',request_id:v.request_id,response:{}}});continue;
 }
 if(v.type==='user'){turns++;if(waiting)continue;send({type:'result',session_id:session,is_error:false,result:'first',usage:{}});continue;}
 assert.equal(v.type,'control_response');assert.equal(v.response.subtype,'success');assert.equal(turns,2,'pending approval must survive until normal turn handling');assert.equal(v.response.request_id,'late-write');if(v.response.response.behavior==='allow')fs.writeFileSync('late.txt','fixture');send({type:'result',session_id:session,is_error:false,result:'second',usage:{}});
}
