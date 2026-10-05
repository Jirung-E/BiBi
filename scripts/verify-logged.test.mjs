import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import path from 'node:path';
import os from 'node:os';
import {fileURLToPath} from 'node:url';

const script = fileURLToPath(new URL('./verify-logged.mjs', import.meta.url));
function workspace(t) {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'bibi verification '));
  t.after(() => rmSync(dir, {recursive: true, force: true}));
  return dir;
}
function execute(log, ...args) {
  return spawnSync(process.execPath, [script, log, ...args], {encoding: 'utf8', timeout: 10_000});
}

test('records both streams and final buffered output without hiding a failing exit', t => {
  const log = path.join(workspace(t), 'logs', 'failed.log');
  const result = execute(log, process.execPath, '-e',
    "process.stdout.write('한글 stdout\\n'+'x'.repeat(256*1024));process.stderr.write('stderr 끝\\n');process.exitCode=7");
  assert.equal(result.status, 7, result.stderr);
  const text = readFileSync(log, 'utf8');
  assert.ok(text.replace(result.stderr, '').includes(result.stdout));
  assert.ok(text.includes(result.stderr));
  assert.equal(result.stdout.length, '한글 stdout\n'.length + 256 * 1024);
  assert.match(text, /Exit code: 7; signal: none/);
});

test('success replaces an older log and preserves argument boundaries', t => {
  const dir = workspace(t), log = path.join(dir, 'run.log');
  writeFileSync(log, 'old failure');
  const result = execute(log, process.execPath, '-e', 'console.log(process.argv[1])', 'space & literal argument');
  assert.equal(result.status, 0, result.stderr);
  const text = readFileSync(log, 'utf8');
  assert.ok(text.includes('space & literal argument'));
  assert.ok(!text.includes('old failure'));
  assert.match(text, /Exit code: 0; signal: none/);
});

test('a missing executable is a recorded failure', t => {
  const dir = workspace(t), log = path.join(dir, 'missing.log');
  const result = execute(log, path.join(dir, 'missing-command'));
  assert.equal(result.status, 1);
  assert.match(readFileSync(log, 'utf8'), /ENOENT/);
});

test('a log that cannot be opened prevents the command from running', t => {
  const dir = workspace(t), marker = path.join(dir, 'should-not-run');
  const result = execute(dir, process.execPath, '-e', "require('node:fs').writeFileSync(process.argv[1],'ran')", marker);
  assert.equal(result.status, 1);
  assert.equal(existsSync(marker), false);
});
