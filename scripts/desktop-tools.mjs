import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync, writeFileSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const frontend = path.join(root, 'gui/frontend');
export const cargo = process.env.CARGO || 'cargo';
export const run = (command, args, cwd = root) => execFileSync(command, args, {cwd, stdio: 'inherit'});

export function prepareDependencies() {
  const cli = path.join(frontend, 'node_modules/@tauri-apps/cli/tauri.js');
  const marker = path.join(frontend, 'node_modules/.bibi-dependencies');
  const hash = createHash('sha256')
    .update(readFileSync(path.join(frontend, 'package-lock.json')))
    .update(readFileSync(path.join(frontend, 'package.json')))
    .update(`${process.platform}/${process.arch}`)
    .digest('hex');
  if (existsSync(cli) && existsSync(marker) && readFileSync(marker, 'utf8') === hash) return cli;
  if (process.platform === 'win32') run(process.env.ComSpec || 'cmd.exe', ['/d', '/s', '/c', 'npm.cmd ci'], frontend);
  else run('npm', ['ci'], frontend);
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
