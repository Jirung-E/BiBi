import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
const [record,mode]=process.argv.slice(2);
if(process.argv.includes('plugin')) {
 fs.appendFileSync(record,JSON.stringify({args:process.argv.slice(4),cwd:process.cwd()})+'\n');
 if(process.argv.includes('list'))console.log(JSON.stringify([{id:'shared@example',scope:'user',enabled:true,installPath:'/fixtures/shared',version:'1.0'}]));
 else console.log('{"success":true}');
 process.exit(0);
}
const write=o=>process.stdout.write(JSON.stringify(o)+'\n');
for await(const line of readline.createInterface({input:process.stdin})){
 const req=JSON.parse(line);fs.appendFileSync(record,JSON.stringify(req)+'\n');
 if(req.id===undefined)continue;
 let result={};
 if(mode==='mcp') {
  if(req.method==='initialize')result={protocolVersion:'2025-03-26',capabilities:{tools:{}},serverInfo:{name:'fixture',version:'1'}};
  else if(req.method==='tools/list')result=req.params.cursor?{tools:[{name:'second'}]}:{tools:[{name:'first',description:'read only probe'}],nextCursor:'next'};
  else {write({jsonrpc:'2.0',id:req.id,error:{code:-32601,message:'only initialize and tools/list allowed'}});continue;}
 } else {
  if(req.method==='config/read')result={config:{mcp_servers:{shared:{command:'shared',env:{TOKEN:'never-expose'}}}},layers:[]};
  else if(req.method==='skills/list'){
   const root=path.join(process.cwd(),'.agents','skills');
   const skills=fs.existsSync(root)?fs.readdirSync(root).filter(n=>fs.existsSync(path.join(root,n,'SKILL.md'))).map(name=>({name,path:path.join(root,name,'SKILL.md'),description:'fixture skill',enabled:true})):[];
   result={data:[{skills,errors:[]}]};
  }else if(req.method==='plugin/list')result={marketplaces:[{plugins:[{id:'shared@example',name:'Shared plugin',installed:true,enabled:true,version:'1'}]}]};
  else if(!['initialize','initialized'].includes(req.method)){write({id:req.id,error:{code:-32601,message:'unsupported'}});continue;}
 }
 write({jsonrpc:'2.0',id:req.id,result});
}
