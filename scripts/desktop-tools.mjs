import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync, writeFileSync, realpathSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const frontend = path.join(root, 'gui/frontend');
export const cargo = process.env.CARGO || 'cargo';
export const run = (command, args, cwd = root) => execFileSync(command, args, {cwd, stdio: 'inherit'});

// Invoke npm's JavaScript CLI directly on Windows; npm.cmd is a batch wrapper.
export function npm(args, cwd = root) {
  if (process.platform !== 'win32') return run('npm', args, cwd);
  const node = path.dirname(realpathSync(process.execPath));
  const candidates = [process.env.npm_execpath,
    path.join(node, 'node_modules/npm/bin/npm-cli.js'),
    ...(process.env.APPDATA ? [path.join(process.env.APPDATA, 'npm/node_modules/npm/bin/npm-cli.js')] : [])];
  const cli = candidates.find(file => file && path.basename(file) === 'npm-cli.js' && existsSync(file));
  if (!cli) throw new Error('Node.js의 npm-cli.js를 찾지 못했습니다. Node.js 설치를 확인하세요.');
  return run(process.execPath, [cli, ...args], cwd);
}

export function prepareDependencies({force = false} = {}) {
  const cli = path.join(frontend, 'node_modules/@tauri-apps/cli/tauri.js');
  const marker = path.join(frontend, 'node_modules/.bibi-dependencies');
  const hash = createHash('sha256')
    .update(readFileSync(path.join(frontend, 'package-lock.json')))
    .update(readFileSync(path.join(frontend, 'package.json')))
    .update(`${process.platform}/${process.arch}`)
    .digest('hex');
  if (!force && existsSync(cli) && existsSync(marker) && readFileSync(marker, 'utf8') === hash) return cli;
  npm(['ci'], frontend);
  writeFileSync(marker, hash);
  return cli;
}

export function tauri(args) {
  const cli = prepareDependencies();
  run(process.execPath, [cli, ...args], path.join(root, 'gui'));
}

export function targetDirectory() {
  return JSON.parse(execFileSync(cargo, ['metadata', '--format-version', '1', '--no-deps'], {cwd: root, encoding: 'utf8'})).target_directory;
}
