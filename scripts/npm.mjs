import {npm} from './desktop-tools.mjs';
try { npm(process.argv.slice(2)); }
catch (error) { console.error(error.message); process.exitCode = error.status || 1; }
