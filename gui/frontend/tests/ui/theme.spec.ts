import {test as base,expect,type Page} from '@playwright/test';
import {sidebarAction,showSidebar} from './navigation';

const test=base.extend<{isolated:void}>({isolated:[async({page,baseURL},use)=>{
 const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));
 await page.route('**/*',route=>{
  const req=route.request();
  if(req.method()==='GET'&&new URL(req.url()).origin===new URL(baseURL!).origin)return route.continue();
  errors.push('unexpected request: '+req.method()+' '+req.url());return route.abort();
 });
 await use();expect(errors).toEqual([]);
},{auto:true}]});
test.beforeEach(async({page},info)=>{
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
});
const conversation='/?view=conversation&project=layout-project&run=layout-parent';

// Composite computed background colors through the real DOM. This covers
// translucent cards/popovers too; decorative gradients are checked visually.
async function contrast(page:Page,selector:string,property='color',minimum=4.5,pseudo:string|null=null){
 const results=await page.locator(selector).evaluateAll((nodes,args)=>{
  const canvas=document.createElement('canvas');canvas.width=canvas.height=1;
  const ctx=canvas.getContext('2d',{willReadFrequently:true})!;
  function rgba(value:string){ctx.clearRect(0,0,1,1);ctx.fillStyle=value;ctx.fillRect(0,0,1,1);return [...ctx.getImageData(0,0,1,1).data].map((v,i)=>i===3?v/255:v);}
  function over(top:number[],bottom:number[]){return top.slice(0,3).map((v,i)=>v*top[3]+bottom[i]*(1-top[3])).concat(1);}
  function background(node:Element|null):number[]{if(!node)return [255,255,255,1];return over(rgba(getComputedStyle(node).backgroundColor),background(node.parentElement));}
  function luminance(rgb:number[]){return rgb.slice(0,3).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4).reduce((s,v,i)=>s+v*[.2126,.7152,.0722][i],0);}
  return nodes.filter(node=>node.getClientRects().length&&!node.matches(':disabled')).map(node=>{
   const style=getComputedStyle(node,args.pseudo),back=background(node);
   const foreground=over(rgba(style.getPropertyValue(args.property)),back);
   const a=luminance(foreground),b=luminance(back);
   return {element:node.tagName+'.'+node.className,text:node.textContent?.slice(0,55),ratio:(Math.max(a,b)+.05)/(Math.min(a,b)+.05)};
  });
 },{property,pseudo});
 expect(results.length,'missing contrast samples: '+selector).toBeGreaterThan(0);
 for(const sample of results)expect(sample.ratio,JSON.stringify(sample)).toBeGreaterThanOrEqual(minimum);
}

for(const width of [390,1280])for(const scheme of ['light','dark'] as const){
 test(`${scheme} surfaces and readable conversation controls at ${width}px`,async({page})=>{
  await page.setViewportSize({width,height:844});await page.emulateMedia({colorScheme:scheme});
  await page.route('**/api/runs/layout-parent',async route=>{
   const response=await route.fetch(),detail=await response.json();
   detail.messages[1].phase='commentary';
   detail.messages[3].text+='\n\n[참고 링크](https://example.com)\n\n> 확인한 내용\n\n인라인 `코드`';
   detail.conversation=detail.messages;await route.fulfill({json:detail});
  });
  await page.goto(conversation);await expect(page.locator('.message')).toHaveCount(24);
  await expect(page.locator('html')).toHaveCSS('color-scheme',scheme);
  const surface=await page.locator('.conversation-panel').evaluate(e=>getComputedStyle(e).backgroundColor);
  expect(surface==='rgb(255, 255, 255)').toBe(scheme==='light');
  await contrast(page,'.message-meta,.messages .markdown p,.messages .markdown code,.messages .markdown th,.messages .markdown td,.conversation-heading .badge,.composer .model-input,.message.commentary summary,.markdown a,.markdown blockquote');
  await contrast(page,'.composer textarea','color',4.5,'::placeholder');
  const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('검증용 초안');
  await contrast(page,'.composer textarea,.composer .primary');
  await page.getByRole('button',{name:'전송',exact:true}).hover();await contrast(page,'.composer .primary');
  if(scheme==='dark'){
   await contrast(page,'.composer textarea,.composer .model-input','border-top-color',3);
   await input.focus();await contrast(page,'.composer textarea','outline-color',3);
  }
  await page.getByRole('button',{name:'최근 모델',exact:true}).click();
  await contrast(page,'.model-history small,.model-history button');
  await page.getByRole('button',{name:'최근 모델',exact:true}).click();
  await showSidebar(page);await contrast(page,'.navigation>button,.sidebar-label,.project-picker,.quota-pill-value');
  await sidebarAction(page,'사용량·연결');await expect(page.locator('.quota-card')).toHaveCount(1);
  await contrast(page,'.quota-card strong,.quota-card small,.quota-card .badge');
  await sidebarAction(page,'설정');const dialog=page.getByRole('dialog',{name:'설정',exact:true});await expect(dialog).toBeVisible();
  await contrast(page,'dialog label,dialog small,dialog .danger-button');
  await dialog.getByRole('button',{name:'제공자 추가',exact:true}).click();
  await dialog.getByRole('button',{name:'Ollama',exact:true}).click();
  await contrast(page,'.provider-editor label,.provider-editor input,.provider-editor select,.provider-editor .primary');
  await dialog.getByRole('button',{name:'닫기',exact:true}).click();
 });
}

for(const width of [390,1280])test(`dark canvas keeps nodes, state colors and focus distinguishable at ${width}px`,async({page})=>{
 await page.setViewportSize({width,height:844});await page.emulateMedia({colorScheme:'dark'});
 await page.route('**/api/snapshot',async route=>{
  const response=await route.fetch(),snapshot=await response.json();
  snapshot.runs.forEach((run:{state:string},i:number)=>run.state=['completed','running','queued','failed','disconnected'][i]);
  await route.fulfill({json:snapshot});
 });
 await page.goto('/?view=canvas&project=layout-project');
 await expect(page.locator('.run-node').first()).toBeVisible();
 // Windows' larger base font can put nodes outside the initial mobile viewport.
 // Fit them before sampling every state; keep normal viewport culling enabled.
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await expect(page.locator('.run-node')).toHaveCount(5);
 await expect(page.locator('html')).toHaveCSS('color-scheme','dark');
 await contrast(page,'.run-node strong,.node-meta,.node-model,.node-bottom,.status-text,.work-group>span,.board-tools button');
 await contrast(page,'.run-node','border-top-color',3);
 const node=page.locator('.run-node').first();await node.click();
 await contrast(page,'.run-node.selected','border-top-color',3);
 await contrast(page,'.run-details .badge,.run-details dt,.run-details dd');
});

test('system appearance switches without reloading or losing a draft',async({page})=>{
 await page.emulateMedia({colorScheme:'light'});await page.goto(conversation);
 const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('계속 작성 중인 질문');
 for(const scheme of ['dark','light','dark'] as const){
  await page.emulateMedia({colorScheme:scheme});await expect(page.locator('html')).toHaveCSS('color-scheme',scheme);
  await expect(input).toHaveValue('계속 작성 중인 질문');await contrast(page,'.composer textarea,.composer .primary');
 }
});

test('saved theme overrides the OS, survives reload, and system mode resumes OS changes',async({page},info)=>{
 await page.setViewportSize({width:390,height:844});await page.emulateMedia({colorScheme:'dark'});
 await page.goto(conversation);
 const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('테마를 바꿔도 남는 초안');
 await sidebarAction(page,'설정');
 const dialog=page.getByRole('dialog',{name:'설정',exact:true}),select=dialog.getByLabel('테마',{exact:true});
 await expect(select).toHaveValue('system');
 await select.selectOption('light');await expect(page.locator('html')).toHaveCSS('color-scheme','light');
 await page.emulateMedia({colorScheme:'light'});await page.emulateMedia({colorScheme:'dark'});
 await expect(page.locator('html')).toHaveCSS('color-scheme','light');
 await expect.poll(()=>page.evaluate(()=>localStorage.getItem('bibi:theme'))).toBe('light');
 await dialog.getByRole('button',{name:'닫기',exact:true}).click();await expect(input).toHaveValue('테마를 바꿔도 남는 초안');
 await page.reload();await expect(page.locator('html')).toHaveCSS('color-scheme','light');
 await sidebarAction(page,'설정');await expect(select).toHaveValue('light');
 await page.emulateMedia({colorScheme:'light'});await select.selectOption('dark');
 await expect(page.locator('html')).toHaveCSS('color-scheme','dark');
 await page.screenshot({path:info.outputPath('manual-dark-theme-mobile.png')});
 await select.selectOption('system');await expect(page.locator('html')).toHaveCSS('color-scheme','light');
 await page.emulateMedia({colorScheme:'dark'});await expect(page.locator('html')).toHaveCSS('color-scheme','dark');
 await expect(select).toHaveValue('system');
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
});

test('saved appearance is applied before the application JavaScript loads',async({page})=>{
 await page.emulateMedia({colorScheme:'light'});
 await page.addInitScript(()=>localStorage.setItem('bibi:theme','dark'));
 let release!:()=>void;
 const gate=new Promise<void>(resolve=>release=resolve);
 await page.route('**/_app/**/*.js',async route=>{await gate;await route.continue();});
 try{
  await page.goto(conversation,{waitUntil:'commit'});
  await expect(page.locator('html')).toHaveAttribute('data-theme','dark');
  await expect(page.locator('html')).toHaveCSS('color-scheme','dark');
  await expect(page.locator('.app-shell')).toHaveCount(0);
 }finally{release();}
 await expect(page.locator('.app-shell')).toBeVisible();
 await expect(page.locator('html')).toHaveCSS('color-scheme','dark');
});

test('invalid or unavailable saved theme falls back to system and manual selection still works',async({page})=>{
 await page.emulateMedia({colorScheme:'dark'});
 await page.addInitScript(()=>{
  localStorage.setItem('bibi:theme','invalid');
  const set=Storage.prototype.setItem;
  Storage.prototype.setItem=function(key,value){if(key==='bibi:theme')throw new DOMException('Storage unavailable','SecurityError');return set.call(this,key,value);};
 });
 await page.goto(conversation);await expect(page.locator('html')).toHaveCSS('color-scheme','dark');
 await sidebarAction(page,'설정');const select=page.getByRole('dialog',{name:'설정',exact:true}).getByLabel('테마',{exact:true});
 await expect(select).toHaveValue('system');await select.selectOption('light');
 await expect(select).toHaveValue('light');await expect(page.locator('html')).toHaveCSS('color-scheme','light');
});

test('theme choice and removal synchronize between tabs without reloading',async({page,context,baseURL})=>{
 await page.emulateMedia({colorScheme:'dark'});await page.goto(conversation);
 const other=await context.newPage();await other.emulateMedia({colorScheme:'light'});await other.goto(baseURL!+conversation);
 try{
  await sidebarAction(page,'설정');const select=page.getByRole('dialog',{name:'설정',exact:true}).getByLabel('테마',{exact:true});
  await select.selectOption('dark');await expect(other.locator('html')).toHaveCSS('color-scheme','dark');
  await other.evaluate(()=>localStorage.removeItem('bibi:theme'));
  await expect(select).toHaveValue('system');await expect(page.locator('html')).toHaveCSS('color-scheme','dark');
 }finally{await other.close();}
});
