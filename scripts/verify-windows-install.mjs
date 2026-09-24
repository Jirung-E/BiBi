import assert from 'node:assert/strict';
import path from 'node:path';
import {run, targetDirectory} from './desktop-tools.mjs';

assert.equal(process.platform, 'win32', 'Installer lifecycle verification requires Windows.');
run('powershell.exe', ['-NoProfile', '-File', 'scripts/verify-install-windows.ps1',
  '-PackageDirectory', path.join(targetDirectory(), 'release/bundle/nsis'),
  '-NodeExecutable', process.execPath]);
