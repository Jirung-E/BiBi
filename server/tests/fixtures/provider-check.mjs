import {appendFileSync} from 'node:fs';
import {createInterface} from 'node:readline';
const [mode,record,...args]=process.argv.slice(2);
const log=value=>appendFileSync(record,JSON.stringify(value)+'\n');
log(args);
if(mode==='hang')setInterval(()=>{},1000);
else if(mode.startsWith('claude')){
 if(JSON.stringify(args)===JSON.stringify(['auth','status','--json']))console.log(JSON.stringify({loggedIn:mode==='claude',email:'must-not-be-returned@example.invalid'}));
 else {
  if(!args.includes('--no-session-persistence')||!args.includes('--print'))process.exit(2);
  createInterface({input:process.stdin}).on('line',line=>{
   const request=JSON.parse(line);log(request.request?.subtype??request.type);
   if(request.type!=='control_request'||request.request.subtype!=='initialize')process.exit(3);
   console.log(JSON.stringify({type:'control_response',response:{subtype:'success',request_id:request.request_id,response:{models:[{value:'sonnet'},{value:'opus'},{value:'sonnet'}]}}}));
  });
 }
}else if(mode.startsWith('codex')){
 if(JSON.stringify(args)!==JSON.stringify(['app-server']))process.exit(2);
 const lines=createInterface({input:process.stdin});
 lines.on('line',line=>{
  const request=JSON.parse(line);log(request.method);
  if(request.method==='initialized')return;
  let result;
  if(request.method==='initialize')result={userAgent:'fixture'};
  else if(request.method==='account/read')result={requiresOpenaiAuth:true,account:mode!=='codex-logged-out'?{type:'chatgpt',email:'must-not-be-returned@example.invalid'}:null};
  else if(request.method==='model/list'){
   if(mode==='codex-no-catalog'){console.log(JSON.stringify({id:request.id,error:{message:'not supported'}}));return;}
   if(request.params.includeHidden!==false)process.exit(4);
   result=request.params.cursor?{data:[{model:'luna'}],nextCursor:null}:{data:[{model:'fixture'},{model:'hidden-system',hidden:true},{model:'fixture'}],nextCursor:'next'};
  }else process.exit(3);
  console.log(JSON.stringify({id:request.id,result}));
 });
}else process.exit(3);
