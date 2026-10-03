import {createHash} from 'node:crypto';
import {existsSync, readFileSync} from 'node:fs';
import {execFileSync, spawnSync} from 'node:child_process';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const [command, ...args] = process.argv.slice(2);
if (!command) throw new Error('Usage: node scripts/verify-source-tree.mjs <command> [args...]');

function snapshot() {
  const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], {
    cwd: root, encoding: 'utf8',
  }).split('\0').filter(Boolean);
  return new Map(files.map(file => {
    const absolute = path.join(root, file);
    const hash = existsSync(absolute)
      ? createHash('sha256').update(readFileSync(absolute)).digest('hex')
      : null;
    return [file, hash];
  }));
}

// Compare working bytes, including existing local edits. Git diff can hide an
// unwanted CRLF/LF rewrite because its clean filter normalizes both versions.
const before = snapshot();
const result = spawnSync(command, args, {cwd: root, stdio: 'inherit'});
const after = snapshot();
const changed = [...new Set([...before.keys(), ...after.keys()])]
  .filter(file => before.get(file) !== after.get(file));
if (changed.length) {
  console.error('Verification changed source files:\n' + changed.map(file => '  ' + file).join('\n'));
  console.error('Keep build outputs out of source files; local edits have been left intact.');
} else {
  console.log('Source files unchanged after verification (' + before.size + ' files).');
}
if (result.error) console.error(result.error.message);
if (result.signal) console.error('Verification terminated by signal: ' + result.signal);
process.exitCode = changed.length || result.error || result.signal ? 1 : (result.status ?? 1);
