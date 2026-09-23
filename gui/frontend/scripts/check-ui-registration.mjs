import fs from 'node:fs';
import { inspectRegistration } from './ui-rules.mjs';
const errors=inspectRegistration({
 manifest:JSON.parse(fs.readFileSync('package.json','utf8')),
 justfile:fs.readFileSync('../../justfile','utf8'),
 workflow:fs.readFileSync('../../.github/workflows/verify.yml','utf8'),
 guards:fs.readdirSync('scripts').filter(file=>/^check-.*\.mjs$/.test(file))
});
if(errors.length){console.error(errors.join('\n'));process.exitCode=1;}else console.log('UI check registration: package → just test → platform CI');
