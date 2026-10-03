import fs from 'node:fs';
import readline from 'node:readline';
const [mode,record]=process.argv.slice(2);
for await(const line of readline.createInterface({input:process.stdin})) {
 const value=JSON.parse(line);fs.appendFileSync(record,JSON.stringify(value)+'\n');
 if(mode==='stall-start')continue;
 const result=value.method==='thread/start'?{thread:{id:'fixture-stop-thread'}}:value.method==='turn/start'?{turn:{id:'fixture-stop-turn'}}:{};
 if(value.id)process.stdout.write(JSON.stringify({id:value.id,result})+'\n');
}
