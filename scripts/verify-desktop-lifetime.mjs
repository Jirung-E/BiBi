import {spawn} from 'node:child_process';
import {mkdtemp, readFile, mkdir, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {setTimeout as sleep} from 'node:timers/promises';

// Real packaged CLI, isolated data, and mock work only. This is also run by
// Windows CI, where console signals cannot cleanly stop a hidden server.
export async function verifyDesktopLifetime(binary) {
  const root = await mkdtemp(path.join(tmpdir(), 'bibi-lifetime-'));
  const workspace = path.join(root, 'workspace');
  await mkdir(workspace);
  let child, exited, exitCode, diagnostic = '', info, token;
  const streamController = new AbortController();
  async function start(managed) {
    exited = false; exitCode = null; diagnostic = '';
    child = spawn(binary, ['--data-dir', root, 'server', '--bind', '127.0.0.1:0', ...(managed ? ['--desktop-managed'] : [])], {stdio: ['pipe', 'ignore', 'pipe'], windowsHide: true});
    child.stderr.on('data', data => { diagnostic = (diagnostic + data).slice(-4000); });
    child.on('exit', code => { exited = true; exitCode = code; });
    child.on('error', error => { diagnostic += error.message; exited = true; });
    for (let attempt = 0; attempt < 100; attempt++) {
      assert.ok(!exited, diagnostic);
      try {
        const current = JSON.parse(await readFile(path.join(root, 'service.json'), 'utf8'));
        if (current.pid === child.pid) {
          info = current;
          token = (await readFile(path.join(root, 'access-token'), 'utf8')).trim();
          return;
        }
      } catch { /* Service metadata is written after startup. */ }
      await sleep(100);
    }
    assert.fail('managed service startup timed out');
  }
  async function request(route, body) {
    const response = await fetch(info.url + route, {method: body ? 'POST' : 'GET', headers: {Authorization: 'Bearer ' + token, ...(body ? {'Content-Type': 'application/json'} : {})}, body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(5000)});
    assert.equal(response.status, 200);
    return response.json();
  }
  try {
    await start(true);
    const project = await request('/api/command', {type: 'create_project', name: 'Desktop lifetime', workspace, constraints: ['Mock only']});
    const run = await request('/api/command', {type: 'submit', request: {submission_id: 'lifetime', project_key: project.id, question: 'Still running '.repeat(200), provider: 'mock', model: 'mock', host_id: 'local', role: 'test', mode: 'fresh', read_only: true}});
    let state;
    for (let i = 0; i < 50; i++) {
      state = (await request('/api/runs/' + run.run_id)).run.state;
      if (state === 'running') break;
      await sleep(100);
    }
    assert.equal(state, 'running');
    const stream = await fetch(info.url + '/api/stream', {headers: {Authorization: 'Bearer ' + token}, signal: streamController.signal});
    assert.equal(stream.status, 200);
    const reader = stream.body.getReader();
    assert.ok(!(await reader.read()).done);
    // Closing the desktop's pipe must interrupt work, persist it, and exit 0.
    child.stdin.end();
    for (let i = 0; i < 150 && !exited; i++) await sleep(100);
    assert.ok(exited, 'desktop-owned server remained alive after pipe EOF');
    assert.equal(exitCode, 0, diagnostic);
    // A connected mobile browser must not hold graceful shutdown open.
    while (!(await reader.read()).done) { /* Drain queued events until EOF. */ }
    await start(false);
    child.stdin.end();
    await sleep(200);
    assert.ok(!exited, 'an explicitly started server must ignore stdin EOF');
    assert.equal((await request('/api/runs/' + run.run_id)).run.state, 'interrupted');
    console.log(JSON.stringify({platform: process.platform, managed_shutdown: true, active_work_interrupted: true, event_stream_closed: true, standalone_server_preserved: true}));
  } finally {
    streamController.abort();
    if (child && !exited) {
      child.kill('SIGINT');
      for (let i = 0; i < 70 && !exited; i++) await sleep(100);
      if (!exited) child.kill('SIGKILL');
      for (let i = 0; i < 10 && !exited; i++) await sleep(100);
    }
    await rm(root, {recursive: true, force: true, maxRetries: 5, retryDelay: 200});
  }
}
