import assert from 'node:assert/strict';
import fs from 'node:fs';
import readline from 'node:readline';
const [mode, record, ...args] = process.argv.slice(2);
const input = readline.createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const receive = async () => {
  const line = await input.next();
  assert(!line.done);
  const value = JSON.parse(line.value);
  fs.appendFileSync(record, JSON.stringify(value) + '\n');
  return value;
};
const send = value => process.stdout.write(JSON.stringify(value) + '\n');
const first = await receive();
assert.equal(first.request.subtype, 'initialize');
if (mode === 'stderr-flood') {
  // More than any platform pipe buffer, with no newline. Must drain throughout.
  await new Promise(resolve => process.stderr.write('x'.repeat(512 * 1024), resolve));
  send({ type: 'control_response', response: { subtype: 'success', request_id: first.request_id, response: {} } });
  assert.equal((await receive()).type, 'user');
  const session = args.find(a => a.startsWith('--session-id=')).split('=')[1];
  send({ type: 'result', session_id: session, is_error: false, result: 'fixture answer', usage: {} });
  await new Promise(() => {});
} else {
  const text = mode === 'old-version'
    ? "error: invalid choice 'manual' for --permission-mode"
    : mode === 'git-bash'
      ? 'Claude Code on Windows requires git-bash'
      : 'private error details';
  await new Promise(resolve => process.stderr.write(text + ' token=must-never-appear\n', resolve));
  process.exit(2);
}
