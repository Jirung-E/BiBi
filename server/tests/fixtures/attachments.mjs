import assert from 'node:assert/strict';
import fs from 'node:fs';
import readline from 'node:readline';
const [adapter,...args]=process.argv.slice(2);
const send=value=>process.stdout.write(JSON.stringify(value)+'\n');
if(adapter==='claude'&&args[0]==='plugin'){send([]);process.exit(0);}
let native=adapter==='claude'?args.find(a=>a.startsWith('--session-id=')||a.startsWith('--resume='))?.split('=')[1]:'attachment-native';
let turns=0;const steering=args.includes('steer');
function validate(items){
 assert(Array.isArray(items));
 const images=items.filter(i=>['localImage','image'].includes(i.type));assert.equal(images.length,1);
 const image=images[0].type==='localImage'?fs.readFileSync(images[0].path):Buffer.from(images[0].source.data,'base64');
 assert.equal(image.subarray(0,8).toString('hex'),'89504e470d0a1a0a');
 if(turns===1){
  assert(items.some(i=>i.text?.includes('ATTACHMENT_FILE_CONTENT')));
  if(adapter==='claude'){const pdf=items.find(i=>i.type==='document');assert.equal(pdf.source.media_type,'application/pdf');assert(Buffer.from(pdf.source.data,'base64').toString().startsWith('%PDF-'));}
  else assert(items.some(i=>i.text?.includes('.pdf')));
  const archive=items.find(i=>i.text?.includes('archive.zip')&&i.text?.includes('Attached file on this host'));assert(archive);const p=JSON.parse(archive.text.split('): ')[1]);assert.equal(fs.readFileSync(p).subarray(0,4).toString(),'PK\x03\x04');
 }
}
for await(const line of readline.createInterface({input:process.stdin})){
 const r=JSON.parse(line);
 if(adapter==='claude'){
  if(r.type==='control_request'){send({type:'control_response',response:{subtype:'success',request_id:r.request_id,response:{commands:[]}}});continue;}
  if(r.type!=='user')continue;
  assert.equal(r.session_id,native);turns++;validate(r.message.content);
  send({type:'system',subtype:'init',session_id:native,model:'fixture'});
  send({type:'assistant',session_id:native,message:{id:'answer-'+turns,content:[{type:'text',text:'attachment answer '+turns}]}});
  send({type:'result',session_id:native,is_error:false,result:'attachment answer '+turns,usage:{}});
 }else{
  if(r.id===undefined)continue;let result={};
  if(r.method==='thread/start'||r.method==='thread/resume')result={thread:{id:native},model:'fixture'};
  if(r.method==='turn/start'){
   assert.equal(r.params.threadId,native);turns++;if(!steering)validate(r.params.input);const id='turn-'+turns;
   if(steering){send({id:r.id,result:{turn:{id,status:'inProgress'}}});continue;}
   send({id:r.id,result:{turn:{id,status:'inProgress'}}});send({method:'item/completed',params:{threadId:native,turnId:id,item:{type:'agentMessage',id:'answer-'+turns,text:'attachment answer '+turns,phase:'final_answer'}}});send({method:'turn/completed',params:{threadId:native,turn:{id,status:'completed'}}});continue;
  }
  if(r.method==='turn/steer'){
   assert(steering);assert.equal(r.params.expectedTurnId,'turn-1');turns=2;validate(r.params.input);
   send({id:r.id,result:{turnId:'turn-1'}});
   send({method:'item/completed',params:{threadId:native,turnId:'turn-1',item:{type:'agentMessage',id:'steered-answer',text:'received steering image',phase:'final_answer'}}});
   send({method:'turn/completed',params:{threadId:native,turn:{id:'turn-1',status:'completed'}}});continue;
  }
  send({id:r.id,result});
 }
}
