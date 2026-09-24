const assert = require('node:assert/strict');
const { appendFileSync } = require('node:fs');
const { createInterface } = require('node:readline');
const args = process.argv.slice(2);
const log = value => appendFileSync(process.env.BIBI_WINDOWS_CLI_RECORD, JSON.stringify(value) + '\n');
if (args[0] === '--echo-args') {
  console.log(JSON.stringify(args.slice(1)));
} else if (__filename.includes('claude-code')) {
  assert.deepEqual(args, ['auth', 'status', '--json']);
  log(args);
  console.log(JSON.stringify({ loggedIn: true }));
} else {
  assert.deepEqual(args, ['app-server']);
  log(args);
  createInterface({ input: process.stdin }).on('line', line => {
    const request = JSON.parse(line);
    log(request.method);
    assert.ok(['initialize', 'initialized', 'account/read'].includes(request.method));
    if (request.method === 'initialized') return;
    const result = request.method === 'initialize'
      ? { userAgent: 'windows-fixture' }
      : { requiresOpenaiAuth: true, account: { type: 'chatgpt' } };
    console.log(JSON.stringify({ id: request.id, result }));
  });
}
