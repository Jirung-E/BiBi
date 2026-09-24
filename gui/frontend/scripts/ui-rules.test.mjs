import { test } from 'node:test';
import assert from 'node:assert/strict';
import { inspectStyles, inspectRegistration } from './ui-rules.mjs';
const palette={'src/tokens.css':':root { --text:#25333b; }'};
const inspect=css=>inspectStyles({...palette,'src/app.css':css});
test('accepts sample tokens, scalable UI dimensions, strokes and dvh',()=>{
 assert.deepEqual(inspect(':root{font-size:14px}.card{color:var(--text);padding:1rem;border:1px solid var(--text);max-height:80vh;max-height:80dvh}'),[]);
});
for(const [name,css,reason] of [
 ['raw color','.card{color:#123456}','color token'],
 ['fixed UI dimension','.card{padding:16px}','use rem/em'],
 ['misspelled token','.card{color:var(--missing,red)}','undefined token'],
 ['old mobile viewport height','.card{height:100vh}','dvh'],
 ['page clipping','.main-content{overflow-x:hidden}','page overflow'],
])test(`rejects ${name}`,()=>assert.ok(inspect(css).some(e=>e.includes(reason))));
test('checks component-local styles and shared dialog locking',()=>{
 assert.ok(inspectStyles({...palette,'src/Dialog.svelte':'<dialog></dialog><style>dialog {padding:20px}</style>'}).length===2);
 assert.deepEqual(inspectStyles({'src/Dialog.svelte':'<script>import {modalDialog} from "./modal";</script><dialog use:modalDialog></dialog>'}),[]);
});
const registered={manifest:{scripts:{'check:ui':'node scripts/check-ui-styles.mjs && node --test scripts/ui-rules.test.mjs','test:ui':'playwright test'}},justfile:'test: build-debug\n    npm --prefix gui/frontend run check:ui\n    npm --prefix gui/frontend run test:ui\n',workflow:'jobs:\n  platform:\n    steps:\n      - run: just test\n',guards:['check-ui-styles.mjs']};
test('accepts a connected package/just/CI check chain',()=>assert.deepEqual(inspectRegistration(registered),[]));
test('rejects an unregistered guard and a disconnected recipe or optional CI',()=>{
 assert.ok(inspectRegistration({...registered,guards:[...registered.guards,'check-forgotten.mjs']}).some(e=>e.includes('check-forgotten')));
 assert.ok(inspectRegistration({...registered,justfile:registered.justfile.replace('run test:ui','test')}).some(e=>e.includes('test:ui')));
 assert.ok(inspectRegistration({...registered,workflow:registered.workflow+'        continue-on-error: true\n'}).length);
});

test('allows only the native window clearances, not fixed application geometry',()=>{
 assert.deepEqual(inspect(':root{--native-titlebar-height:56px;--native-controls-width:96px}'),[]);
 assert.ok(inspect(':root{--native-controls-width:120px}').some(e=>e.includes('use rem/em')));
 assert.ok(inspect('.card{width:96px}').some(e=>e.includes('use rem/em')));
});
