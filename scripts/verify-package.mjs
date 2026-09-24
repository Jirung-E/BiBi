import assert from 'node:assert/strict';
import {access, mkdtemp, readdir, rm} from 'node:fs/promises';
import {constants} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {run, tauri, targetDirectory} from './desktop-tools.mjs';

const bundleType = {darwin: 'app', linux: 'deb', win32: 'nsis'}[process.platform];
assert.ok(bundleType, 'Unsupported package platform: ' + process.platform);
tauri(['build', '--ci', '--bundles', bundleType, '--', '--locked']);
const release = path.join(targetDirectory(), 'release');
const bundles = path.join(release, 'bundle');
const smoke = binary => run(process.execPath, ['scripts/verify-service.mjs', binary]);

if (process.platform === 'darwin') {
  const app = path.join(bundles, 'macos', 'BiBi.app');
  await access(path.join(app, 'Contents/MacOS/bibi-desktop'), constants.X_OK);
  run('codesign', ['--verify', '--deep', '--strict', app]);
  smoke(path.join(app, 'Contents/MacOS/bibi'));
  const arch = {arm64: 'ARM64', x64: 'X64'}[process.arch] || process.arch;
  run('tar', ['-czf', path.join(bundles, 'BiBi-macos-' + arch + '.tar.gz'), '-C', path.join(bundles, 'macos'), 'BiBi.app']);
} else if (process.platform === 'linux') {
  const folder = path.join(bundles, 'deb');
  const packages = (await readdir(folder)).filter(name => name.endsWith('.deb'));
  assert.equal(packages.length, 1, 'Expected one Debian package');
  const extracted = await mkdtemp(path.join(tmpdir(), 'bibi-package-'));
  try {
    run('dpkg-deb', ['--extract', path.join(folder, packages[0]), extracted]);
    await access(path.join(extracted, 'usr/bin/bibi-desktop'), constants.X_OK);
    smoke(path.join(extracted, 'usr/bin/bibi'));
  } finally {
    await rm(extracted, {recursive: true, force: true});
  }
} else {
  const installers = (await readdir(path.join(bundles, 'nsis'))).filter(name => name.endsWith('-setup.exe'));
  assert.equal(installers.length, 1, 'Expected one NSIS installer');
  await access(path.join(release, 'bibi-desktop.exe'));
  smoke(path.join(release, 'bibi.exe'));
  // Real installation changes per-user registry entries and shortcuts.
  // Keep that explicit so normal local tests cannot replace an installed BiBi.
  console.log('NSIS built. Real installation lifecycle: just test-install (clean Windows account).');
}
console.log('Package verification passed: ' + bundleType);
