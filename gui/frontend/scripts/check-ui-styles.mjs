import fs from 'node:fs';
import path from 'node:path';
import { inspectStyles } from './ui-rules.mjs';
function files(dir){return fs.readdirSync(dir,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?files(path.join(dir,entry.name)):[path.join(dir,entry.name)]);}
const sources=Object.fromEntries(files('src').filter(file=>/\.(css|svelte)$/.test(file)).map(file=>[file.replaceAll('\\','/'),fs.readFileSync(file,'utf8')]));
const errors=inspectStyles(sources);
if(errors.length){console.error(errors.join('\n'));process.exitCode=1;}else console.log(`UI styles: ${Object.keys(sources).length} files checked`);
