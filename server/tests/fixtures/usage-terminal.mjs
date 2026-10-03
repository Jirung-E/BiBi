import fs from 'node:fs';
const [record,mode]=process.argv.slice(2);
if(process.stdin.isTTY)process.stdin.setRawMode(true);
if(mode==='trust')process.stdout.write('Do you trust this folder?');
else process.stdout.write('Claude Code\r\n? for shortcuts\r\n❯ ');
let input='';process.stdin.on('data',buffer=>{
 input+=buffer.toString();if(!input.includes('\r'))return;
 fs.writeFileSync(record,input);if(input.trim()!=='/usage')process.exit(9);
 const draw=()=>{
  // Reproduce ConPTY's split redraw on every host, including an empty screen.
  process.stdout.write('\x1b[2J\x1b[H');
  setTimeout(()=>process.stdout.write('Current session\r\n27% used\r\n'),10);
  setTimeout(()=>process.stdout.write('Current week (all models)\r\n62% used\r\nEsc to close'),25);
 };
 draw();if(mode==='redraw')setInterval(draw,80);
});
setInterval(()=>{},1000);
