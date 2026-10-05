import fs from 'node:fs';
import path from 'node:path';
const [record, mode] = process.argv.slice(2);
if (process.stdin.isTTY) process.stdin.setRawMode(true);
const ready = () => process.stdout.write('Claude Code\r\n? for shortcuts\r\n❯ ');
let input = '';
let waitingForCursor = mode === 'cursor-query';
process.stdin.on('data', buffer => {
  input += buffer.toString();
  if (waitingForCursor) {
    const cursor = input.match(/^\x1b\[\d+;\d+R/);
    if (!cursor) return;
    fs.writeFileSync(path.join(path.dirname(record), 'input.cursor'), cursor[0]);
    if (cursor[0] !== '\x1b[4;7R') process.exit(8);
    input = input.slice(cursor[0].length);
    waitingForCursor = false;
    ready();
  }
  if (!input.includes('\r')) return;
  fs.writeFileSync(record, input);
  if (input !== '/usage\r') process.exit(9);
  input = '';
  const draw = () => {
    // Split the clear and redraw even on hosts without ConPTY.
    process.stdout.write('\x1b[2J\x1b[H');
    setTimeout(() => process.stdout.write('Current session\r\n27% used\r\n'), 10);
    setTimeout(() => process.stdout.write('Current week (all models)\r\n62% used\r\nEsc to close'), 25);
  };
  draw();
  if (mode === 'redraw') setInterval(draw, 80);
});
if (mode === 'trust') process.stdout.write('Do you trust this folder?');
else if (waitingForCursor) {
  process.stdout.write('\x1b[4;7H\x1b[');
  setTimeout(() => process.stdout.write('6n'), 40);
} else ready();
setInterval(() => {}, 1000);
