import postcss from 'postcss';
import { parse } from 'svelte/compiler';
import { parse as yaml } from 'yaml';

export function inspectStyles(sources) {
  const errors=[], sheets=[], definitions=new Set();
  const report=(file,line,message)=>errors.push(`${file}:${line}: ${message}`);
  for(const [file,source] of Object.entries(sources)) {
    try {
      if(file.endsWith('.css')) sheets.push({file,root:postcss.parse(source,{from:file}),offset:0});
      if(file.endsWith('.svelte')) {
        const ast=parse(source,{modern:true});
        if(ast.css) sheets.push({file,root:postcss.parse(ast.css.content.styles,{from:file}),offset:source.slice(0,ast.css.content.start).split('\n').length-1});
        const visit=node=>{
          if(!node||typeof node!=='object')return;
          if(node.type==='RegularElement'&&node.name==='dialog'&&!node.attributes.some(a=>a.type==='UseDirective'&&a.name==='modalDialog'))
            report(file,source.slice(0,node.start).split('\n').length,'native dialogs must use the shared modalDialog scroll lock');
          for(const value of Object.values(node)) if(Array.isArray(value))value.forEach(visit);else if(value&&typeof value==='object')visit(value);
        };
        visit(ast.fragment);
      }
    } catch(error) { report(file,error.line??1,error.message); }
  }
  for(const {root} of sheets)root.walkDecls(d=>{if(d.prop.startsWith('--'))definitions.add(d.prop);});
  for(const {file,root,offset} of sheets)root.walkDecls(d=>{
    const fail=message=>report(file,(d.source?.start?.line??1)+offset,message);
    const selector=d.parent.selector??'';
    if(/#[\da-f]{3,8}\b|\b(?:rgb|hsl)a?\(/i.test(d.value)&&!(file==='src/tokens.css'&&selector===':root'&&d.prop.startsWith('--')))
      fail('use a color token from src/tokens.css');
    for(const [,name] of d.value.matchAll(/var\(\s*(--[\w-]+)/g))if(!definitions.has(name))fail(`undefined token ${name}`);
    // Physical strokes and paint stay crisp. Canvas JS coordinates are tested
    // in the browser; they must not be mechanically converted into rem.
    const paint=/^(?:border(?:-(?:top|right|bottom|left))?(?:-width)?|outline(?:-width)?|box-shadow|text-shadow|background(?:-image)?|backdrop-filter)$/;
    const base=selector===':root'&&d.prop==='font-size'&&d.value==='14px';
    const accessibility=selector==='.sr-only'&&/^(?:width|height|margin)$/.test(d.prop)&&/^-?1px$/.test(d.value);
    // macOS window buttons do not follow the app's 50–200% font scale.
    // Only these two native clearances may use fixed logical pixels.
    const nativeChrome=selector===':root'&&(
      d.prop==='--native-titlebar-height'&&d.value==='56px'||
      d.prop==='--native-controls-width'&&d.value==='96px');
    const svgOrigin=selector==='.connections'&&/^(?:width|height)$/.test(d.prop)&&d.value==='1px';
    if(/-?[\d.]+px\b/.test(d.value)&&!paint.test(d.prop)&&!base&&!accessibility&&!svgOrigin&&!nativeChrome)
      fail(`${d.prop}: use rem/em for UI dimensions; keep physical pixels only for paint or canvas coordinates`);
    if(/^(?:min-|max-)?(?:height|block-size)$/.test(d.prop)&&/\d(?:\.\d+)?vh\b/.test(d.value)){
      const siblings=d.parent.nodes, index=siblings.indexOf(d);
      if(!siblings.slice(index+1).some(n=>n.prop===d.prop&&/dvh\b/.test(n.value)))fail('viewport height needs a following dvh fallback override');
    }
    if(/^overflow(?:-[xy])?$/.test(d.prop)&&/hidden|clip/.test(d.value)&&selector.split(',').some(s=>/^(?:html|body|:root|\.app-shell|\.main-content)$/.test(s.trim())))
      fail('do not hide page overflow to conceal layout errors; modalDialog owns temporary page locking');
  });
  return errors;
}

export function inspectRegistration({manifest,justfile,workflow,guards}) {
  const errors=[], scripts=manifest.scripts??{};
  for(const file of guards)if(!(scripts['check:ui']??'').includes(`node scripts/${file}`))errors.push(`${file} is not registered in check:ui`);
  if(!/node --test scripts\//.test(scripts['check:ui']??''))errors.push('check:ui must exercise its own negative fixtures');
  if(scripts['test:ui']!=='playwright test')errors.push('test:ui must run the browser suite');
  const recipe=justfile.match(/^test(?:[^\n]*):[^\n]*\n((?:[ \t]+[^\n]*\n|\n)*)/m)?.[1]??'';
  for(const name of ['check:ui','test:ui'])if(!recipe.split('\n').some(line=>line.trim()===`npm --prefix gui/frontend run ${name}`))errors.push(`just test does not run ${name}`);
  for(const command of ['just build-debug','cargo test --workspace --locked','cargo clippy --workspace --all-targets --locked -- -D warnings','node scripts/verify-service.mjs','just build','node scripts/verify-package.mjs'])
    if(!recipe.split('\n').some(line=>line.trim()===command))errors.push('just test must include '+command);
  const steps=yaml(workflow)?.jobs?.platform?.steps??[];
  if(!steps.some(step=>step.run==='just test'&&!step.if&&!step['continue-on-error']))errors.push('platform CI must run just test without an optional condition');
  if(steps.some(step=>step.run==='just build'||step.run?.includes('node scripts/tauri.mjs build')||step.run?.includes('node scripts/verify-package.mjs')))
    errors.push('CI build/package verification belongs in just test, not a CI-only step');
  if(!steps.some(step=>step.run==='just test-install'&&step.if==="runner.os == 'Windows'"&&!step['continue-on-error']))
    errors.push('Windows CI must run the shared test-install recipe');
  return errors;
}
