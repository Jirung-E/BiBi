import assert from 'node:assert/strict';
import fs from 'node:fs';
const [record, mode, ...args]=process.argv.slice(2);
fs.appendFileSync(record,JSON.stringify(args)+'\n');
if(args.includes('--help')) {console.log(mode==='legacy'?'Launch IDE':'--print --output-format --conversation');process.exit(0);}
assert.equal(args[args.indexOf('--output-format')+1],'stream-json');
assert(args.includes('--print'));assert(!args.includes('--dangerously-skip-permissions'));
const previous=args.indexOf('--conversation');
const session=previous>=0?args[previous+1]:'ac4b755c-d504-45ee-bc92-badf804a0d88';
const emit=event=>process.stdout.write(JSON.stringify(event)+'\n');
emit({event:'init',conversation_id:session,init:{model:'fixture-agy'}});
if(mode==='hang'){setInterval(()=>{},1000);await new Promise(()=>{});}
emit({event:'step_update',step_update:{step_type:'agent_response',text_delta:'한글 '+(previous>=0?'이어서':'처음')}});
emit({event:'result',result:{status:'SUCCESS',conversation_id:session,response:'한글 '+(previous>=0?'이어서':'처음'),usage:{input_tokens:10,output_tokens:3}}});
