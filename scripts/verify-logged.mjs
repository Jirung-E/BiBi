import {mkdirSync, openSync, closeSync, writeFileSync} from 'node:fs';
import {spawn} from 'node:child_process';
import path from 'node:path';

const [logPath, command, ...args] = process.argv.slice(2);
if (!logPath || !command) throw new Error('Usage: node scripts/verify-logged.mjs <log> <command> [args...]');

mkdirSync(path.dirname(logPath), {recursive: true});
const log = openSync(logPath, 'w');
const record = value => writeFileSync(log, value);
record('Verification started: ' + new Date().toISOString() + '\n');
const child = spawn(command, args, {stdio: ['inherit', 'pipe', 'pipe'], windowsHide: true});
let failure;
const interrupt = () => child.kill('SIGINT');
const terminate = () => child.kill('SIGTERM');
process.on('SIGINT', interrupt);
process.on('SIGTERM', terminate);
child.stdout.on('data', chunk => {record(chunk); process.stdout.write(chunk);});
child.stderr.on('data', chunk => {record(chunk); process.stderr.write(chunk);});
child.on('error', error => {
  failure = error;
  record(error.message + '\n');
  console.error(error.message);
});
const result = await new Promise(resolve => child.on('close', (code, signal) => resolve({code, signal})));
process.removeListener('SIGINT', interrupt);
process.removeListener('SIGTERM', terminate);
record('\nVerification finished: ' + new Date().toISOString() + '\n');
record('Exit code: ' + result.code + '; signal: ' + (result.signal ?? 'none') + '\n');
closeSync(log);
// A successful tee must never turn a failed build/test command into success.
process.exitCode = failure || result.signal ? 1 : (result.code ?? 1);
