const assert = require('node:assert/strict');
const { appendFileSync } = require('node:fs');
const { createInterface } = require('node:readline');
const args = process.argv.slice(2);
const log = value => appendFileSync(process.env.BIBI_WINDOWS_CLI_RECORD || __filename + '.record.jsonl', JSON.stringify(value) + '\n');
if (args[0] === '--echo-args') {
  console.log(JSON.stringify(args.slice(1)));
} else if (__filename.includes('claude-code')) {
  log(args);
  if (args[0] === 'auth') {
    assert.deepEqual(args, ['auth', 'status', '--json']);
    console.log(JSON.stringify({ loggedIn: true }));
  } else {
    assert.deepEqual(args, ['--print', '--input-format', 'stream-json', '--output-format', 'stream-json', '--verbose', '--permission-prompt-tool', 'stdio', '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}', '--tools', '', '--no-session-persistence']);
    createInterface({ input: process.stdin }).on('line', line => {
      const request = JSON.parse(line);
      assert.equal(request.type, 'control_request');
      assert.equal(request.request.subtype, 'initialize');
      log(request.request.subtype);
      console.log(JSON.stringify({ type: 'control_response', response: { subtype: 'success', request_id: request.request_id, response: { models: [{ value: 'sonnet' }] } } }));
    });
  }
} else {
  assert.deepEqual(args, ['app-server']);
  log(args);
  createInterface({ input: process.stdin }).on('line', line => {
    const request = JSON.parse(line);
    log(request.method);
    assert.ok(['initialize', 'initialized', 'account/read', 'model/list'].includes(request.method));
    if (request.method === 'initialized') return;
    if (request.method === 'model/list') assert.equal(request.params.includeHidden, false);
    const result = request.method === 'model/list'
      ? { data: [{ model: 'fixture-codex' }], nextCursor: null }
      : request.method === 'initialize'
      ? { userAgent: 'windows-fixture' }
      : { requiresOpenaiAuth: true, account: { type: 'chatgpt' } };
    console.log(JSON.stringify({ id: request.id, result }));
  });
}
