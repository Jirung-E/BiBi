import {appendFileSync} from 'node:fs';
import {createInterface} from 'node:readline';
const [mode,record,...args]=process.argv.slice(2);
const log=value=>appendFileSync(record,JSON.stringify(value)+'\n');
log(args);
if(mode==='hang')setInterval(()=>{},1000);
else if(mode.startsWith('claude')){
 if(JSON.stringify(args)!==JSON.stringify(['auth','status','--json']))process.exit(2);
 console.log(JSON.stringify({loggedIn:mode==='claude',email:'must-not-be-returned@example.invalid'}));
}else if(mode.startsWith('codex')){
 if(JSON.stringify(args)!==JSON.stringify(['app-server','--stdio']))process.exit(2);
 const lines=createInterface({input:process.stdin});
 lines.on('line',line=>{
  const request=JSON.parse(line);log(request.method);
  if(request.method==='initialized')return;
  const result=request.method==='initialize'?{userAgent:'fixture'}:request.method==='account/read'?{requiresOpenaiAuth:true,account:mode==='codex'?{type:'chatgpt',email:'must-not-be-returned@example.invalid'}:null}:null;
  console.log(JSON.stringify({id:request.id,result}));
 });
}else process.exit(3);
