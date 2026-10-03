import fs from 'node:fs';
const [record,mode]=process.argv.slice(2);
if(process.stdin.isTTY)process.stdin.setRawMode(true);
if(mode==='trust')process.stdout.write('Do you trust this folder?');
else process.stdout.write('Claude Code\r\n? for shortcuts\r\n❯ ');
let input='';process.stdin.on('data',buffer=>{
 input+=buffer.toString();if(!input.includes('\r'))return;
 fs.writeFileSync(record,input);if(input.trim()!=='/usage')process.exit(9);
 const draw=()=>process.stdout.write('\x1b[2J\x1b[HCurrent session\r\n27% used\r\nCurrent week (all models)\r\n62% used\r\nEsc to close');
 draw();if(mode==='redraw')setInterval(draw,80);
});
setInterval(()=>{},1000);
