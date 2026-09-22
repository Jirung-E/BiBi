import {tauri} from './desktop-tools.mjs';

try {
  tauri(process.argv.slice(2));
} catch (error) {
  console.error(error.message);
  process.exitCode = typeof error.status === 'number' ? error.status || 1 : 1;
}
