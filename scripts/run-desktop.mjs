import {existsSync, realpathSync} from 'node:fs';
import path from 'node:path';
import {run, targetDirectory, tauri} from './desktop-tools.mjs';

try {
  const profileDir = realpathSync(process.argv[2]);
  const relative = path.relative(realpathSync(targetDirectory()), profileDir).split(path.sep);
  const profile = relative.at(-1);
  if (relative.some(part => part === '..') || relative.length > 2 || !['debug', 'release'].includes(profile)) {
    throw new Error('cargo run 또는 cargo run --release로 저장소에서 실행하세요.');
  }
  const args = process.argv.slice(3);
  const check = args.length === 1 && args[0] === '--check';
  const build = ['build', '--no-bundle', '--ci'];
  if (profile === 'debug') build.push('--debug');
  if (relative.length === 2) build.push('--target', relative[0]);
  build.push('--', '--locked');
  tauri(build);
  const suffix = process.platform === 'win32' ? '.exe' : '';
  const server = path.join(profileDir, 'bibi' + suffix);
  const desktop = path.join(profileDir, 'bibi-desktop' + suffix);
  if (!existsSync(server) || !existsSync(desktop)) throw new Error('서버와 데스크톱 실행 파일을 함께 빌드하지 못했습니다.');
  if (check) {
    run(server, ['--version']);
    console.log(JSON.stringify({source_build: true, profile, server, desktop}));
  } else {
    run(server, args);
  }
} catch (error) {
  console.error(error.message);
  process.exitCode = typeof error.status === 'number' ? error.status || 1 : 1;
}
