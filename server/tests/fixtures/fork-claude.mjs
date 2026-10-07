import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const args=process.argv.slice(2),send=value=>process.stdout.write(JSON.stringify(value)+'\n');
const native=args.find(a=>a.startsWith('--resume='))?.slice(9);
assert(native,'fork followup must resume a saved new native ID');assert(!args.includes('--fork-session'));
const root=process.env.CLAUDE_CONFIG_DIR;
const file=path.join(root,'projects','fixture',native+'.jsonl');
const rows=fs.readFileSync(file,'utf8').trim().split('\n').map(JSON.parse);
assert(rows.every(r=>r.sessionId===native));
for await(const raw of readline.createInterface({input:process.stdin})){
 const value=JSON.parse(raw);
 if(value.type==='control_request'){
  send({type:'control_response',response:{subtype:'success',request_id:value.request_id,response:{commands:[]}}});continue;
 }
 assert.equal(value.type,'user');assert.equal(value.session_id,native);
 const text=JSON.stringify({native,history:rows.filter(r=>r.type==='user'||r.type==='assistant'),question:value.message.content});
 fs.appendFileSync(path.join(root,'questions'),text+'\n');
 send({type:'system',subtype:'init',session_id:native,model:'fixture-model',transcript_path:file});
 send({type:'assistant',session_id:native,uuid:randomUUID(),message:{id:randomUUID(),content:[{type:'text',text}]}});
 send({type:'result',session_id:native,is_error:false,result:text,usage:{},duration_ms:1});
}
