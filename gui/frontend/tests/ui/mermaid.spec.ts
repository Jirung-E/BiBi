import {test,expect,type Page,type TestInfo} from '@playwright/test';

const fence=(source:string)=>'\x60\x60\x60mermaid\n'+source+'\n\x60\x60\x60';
const flow='flowchart LR\n  U[사용자] --> B[BiBi]\n  B --> C[전문가]';
const sequence='sequenceDiagram\n  participant U as 사용자\n  participant B as BiBi\n  U->>B: 업무 요청\n  B-->>U: 결과';
async function fixture(page:Page,baseURL:string,info:TestInfo,texts:()=>string[]){
 const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
 let revision=0,emit!:()=>void;
 let update=new Promise<void>(resolve=>emit=resolve);
 await page.route('**/*',async route=>{
  const url=new URL(route.request().url());
  if(url.origin!==new URL(baseURL).origin||route.request().method()!=='GET'){errors.push('unexpected request: '+url.origin);return route.abort();}
  if(url.pathname==='/api/stream'){
   if(revision<=Number(url.searchParams.get('after')??0))await update;
   return route.fulfill({contentType:'text/event-stream',body:'event: update\ndata: '+JSON.stringify({seq:revision,kind:'message',data:{run_id:'layout-parent'},id:'diagram-'+revision,created_at:Date.now()})+'\n\n'});
  }
  if(url.pathname==='/api/runs/layout-parent'){
   const response=await route.fetch(),data=await response.json(),example=data.messages.at(-1);
   data.messages=texts().map((text,i)=>({...example,id:'diagram-message-'+i,text,created_at:example.created_at+i}));
   data.conversation=data.messages;
   return route.fulfill({response,json:data});
  }
  return route.continue();
 });
 return {errors,send(){const next=emit;revision++;update=new Promise<void>(resolve=>emit=resolve);next();}};
}
const open=async(page:Page)=>page.goto('/?view=conversation&project=layout-project&run=layout-parent');
async function loaded(page:Page,index=0){
 const block=page.locator('.mermaid-block').nth(index);
 await page.getByLabel('대화 기록',{exact:true}).dispatchEvent('wheel',{deltaY:-1});
 await block.locator('xpath=ancestor::article[1]').scrollIntoViewIfNeeded();
 await block.evaluate(e=>e.scrollIntoView({block:'center'}));
 await expect(block).toBeInViewport();
 await expect(block.locator('.mermaid-preview img')).toBeVisible();
 await expect.poll(()=>block.locator('img').evaluate((e:HTMLImageElement)=>e.complete&&e.naturalWidth>0)).toBe(true);
 return block;
}

test('Mermaid previews flowcharts and sequences with independent keyboard-accessible source switches',async({page,baseURL},info)=>{
 const text='## 업무 흐름\n\n'+fence(flow)+'\n\n> '+fence(sequence).replaceAll('\n','\n> ')+'\n\n\x60\x60\x60ts\nconst diagram = "plain code";\n\x60\x60\x60';
 const {errors}=await fixture(page,baseURL!,info,()=>[text]);
 await page.setViewportSize({width:1440,height:1000});await open(page);
 const first=await loaded(page),second=await loaded(page,1);
 await expect(page.locator('.markdown blockquote .mermaid-block')).toHaveCount(1);
 await expect(page.locator('code.language-ts')).toHaveText('const diagram = "plain code";');
 await first.getByRole('button',{name:'코드',exact:true}).focus();await page.keyboard.press('Enter');
 await expect(first.locator('pre')).toHaveText(flow);
 await expect(first.getByRole('button',{name:'코드',exact:true})).toHaveAttribute('aria-pressed','true');
 await expect(second.locator('img')).toBeVisible();
 await first.getByRole('button',{name:'미리보기',exact:true}).click();await loaded(page);
 await page.getByLabel('대화 기록',{exact:true}).evaluate(e=>e.scrollTo(0,0));
 await page.screenshot({path:info.outputPath('mermaid-desktop.png')});
 expect(errors).toEqual([]);
});

test('invalid, partial and unsafe diagrams retain source without affecting the app or loading external resources',async({page,baseURL},info)=>{
 const sources=[
  'not a valid diagram',
  'flowchart LR\n A[',
  '%%{init: {"securityLevel":"loose"}}%%\nflowchart LR\n A-->B',
  'flowchart LR\n A@{img:"https://example.invalid/track.png"}',
  'flowchart LR\n A["<img src=x onerror=alert(1)>"]',
  'flowchart LR\n A-->B\n classDef default fill:url(https://example.invalid/style)',
  'flowchart LR\n A-->B\n classDef default fill:u\\72l(https://example.invalid/escaped)',
  'flowchart LR\n A["'+'large'.repeat(4100)+'"]'
 ];
 const {errors}=await fixture(page,baseURL!,info,()=>sources.map(fence));
 await open(page);
 for(let i=0;i<sources.length;i++){
  const block=page.locator('.mermaid-block').nth(i);
  await page.getByLabel('대화 기록',{exact:true}).dispatchEvent('wheel',{deltaY:-1});
  await block.locator('xpath=ancestor::article[1]').scrollIntoViewIfNeeded();await block.scrollIntoViewIfNeeded();
  await expect(block.locator('.mermaid-notice')).toBeVisible();
  await block.getByRole('button',{name:'코드 보기',exact:true}).click();
  await expect(block.locator('pre')).toHaveText(sources[i]);
 }
 await expect(page.locator('body > [aria-hidden="true"] svg')).toHaveCount(0);
 await expect(page.locator('.markdown script,.markdown iframe,.markdown [onerror]')).toHaveCount(0);
 expect(errors).toEqual([]);
});

test('stream updates preserve source choice, recover incomplete diagrams and reuse rendered previews',async({page,baseURL},info)=>{
 let text=fence(flow);
 const state=await fixture(page,baseURL!,info,()=>[text]);await open(page);
 const block=await loaded(page),original=await block.locator('img').getAttribute('src');
 await block.getByRole('button',{name:'코드',exact:true}).click();
 text='설명 추가\n\n'+fence(flow+'\n C --> R[완료]');state.send();
 await expect(block.locator('pre')).toContainText('완료');
 await expect(block.getByRole('button',{name:'코드',exact:true})).toHaveAttribute('aria-pressed','true');
 await block.getByRole('button',{name:'미리보기',exact:true}).click();await loaded(page);
 const changed=await block.locator('img').getAttribute('src');expect(changed).not.toBe(original);
 text+='\n\n후속 설명';state.send();await expect(page.locator('.markdown')).toContainText('후속 설명');
 await loaded(page);await expect(block.locator('img')).toHaveAttribute('src',changed!);
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 await expect(block.locator('img')).toHaveAttribute('src',changed!);
 text=fence('flowchart LR\n X[');state.send();await expect(block.locator('.mermaid-notice')).toBeVisible();
 text=fence('flowchart LR\n X[복원] --> Y[성공]');state.send();await loaded(page);
 expect(decodeURIComponent((await block.locator('img').getAttribute('src'))!)).toContain('복원');
 expect(state.errors).toEqual([]);
});

for(const [width,scale] of [[390,1],[320,2]])test('mobile diagram controls and wide previews remain usable at '+width+' / '+scale,async({page,baseURL},info)=>{
 await page.setViewportSize({width,height:844});await page.emulateMedia({colorScheme:'dark'});
 await page.addInitScript(value=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:value})),scale);
 const wide='flowchart LR\n'+Array.from({length:9},(_,i)=>'N'+i+'[단계 '+i+'] --> N'+(i+1)).join('\n');
 const {errors}=await fixture(page,baseURL!,info,()=>[fence(wide)]);await open(page);
 const block=await loaded(page),viewport=block.locator('.mermaid-preview');
 await page.getByLabel('대화 기록',{exact:true}).dispatchEvent('wheel',{deltaY:-1});
 await viewport.scrollIntoViewIfNeeded();await expect(viewport).toBeInViewport();
 await expect(viewport).toHaveAttribute('data-overlay-scrollbars','');
 const geometry=await page.evaluate(()=>({page:document.documentElement.scrollWidth,width:innerWidth,block:document.querySelector('.mermaid-block')!.getBoundingClientRect().width}));
 expect(geometry.page).toBeLessThanOrEqual(width+1);expect(geometry.block).toBeLessThan(width);
 expect(await viewport.evaluate(e=>e.scrollWidth>e.clientWidth)).toBe(true);
 const thumb=page.locator('.overlay-thumb.horizontal[aria-controls="'+await viewport.getAttribute('id')+'"]');
 await expect(thumb).toBeVisible();await thumb.focus();await page.keyboard.press('End');
 expect(await viewport.evaluate(e=>e.scrollLeft)).toBeGreaterThan(0);
 const dark=await block.locator('img').getAttribute('src');
 await page.emulateMedia({colorScheme:'light'});await expect(block.locator('img')).not.toHaveAttribute('src',dark!);
 await loaded(page);
 await block.getByRole('button',{name:'코드',exact:true}).click();
 await expect(block.locator('pre')).toHaveText(wide);
 await page.emulateMedia({colorScheme:'dark'});await expect(block.locator('pre')).toBeVisible();
 await block.getByRole('button',{name:'미리보기',exact:true}).click();await loaded(page);
 await page.screenshot({path:info.outputPath('mermaid-mobile-dark.png')});
 expect(errors).toEqual([]);
});

test('a long diagram history renders only nearby blocks and keeps an older preview stable through panel changes',async({page,baseURL},info)=>{
 const {errors}=await fixture(page,baseURL!,info,()=>Array.from({length:120},(_,i)=>fence('flowchart LR\n A[업무 '+i+'] --> B[완료]')));
 await page.setViewportSize({width:1440,height:900});await open(page);
 await expect(page.locator('.mermaid-block')).toHaveCount(120);
 await loaded(page,119);
 expect(await page.locator('.mermaid-preview img').count()).toBeLessThan(12);
 await page.getByLabel('대화 기록',{exact:true}).dispatchEvent('wheel',{deltaY:-1});
 const older=await loaded(page,20),uri=await older.locator('img').getAttribute('src');
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 await expect(older).toBeInViewport();
 await expect(older.locator('img')).toHaveAttribute('src',uri!);
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 await expect(older).toBeInViewport();
 await expect.poll(()=>page.locator('.mermaid-preview img').count()).toBeLessThan(12);
 expect(errors).toEqual([]);
});

test('plain Markdown loads the diagram engine only after a Mermaid response arrives',async({page,baseURL},info)=>{
 const scripts:Promise<string>[]=[];
 page.on('response',response=>{if(new URL(response.url()).pathname.endsWith('.js'))scripts.push(response.text());});
 let text='# 제목\n\n\x60\x60\x60js\nconsole.log("hello")\n\x60\x60\x60';
 const state=await fixture(page,baseURL!,info,()=>[text]);
 await open(page);await expect(page.locator('code.language-js')).toBeVisible();
 // Bundles have hashed names. Check the engine's exported API in response
 // bodies, not the original npm filename which never appears in these URLs.
 const engines=async()=>(await Promise.all(scripts)).filter(body=>body.includes('registerIconPacks')&&body.includes('mermaidAPI')).length;
 expect(await engines()).toBe(0);
 text=fence(flow);state.send();await loaded(page);
 await expect.poll(engines).toBeGreaterThan(0);
 expect(state.errors).toEqual([]);
});

test('class, state and pie diagrams render as local images',async({page,baseURL},info)=>{
 const sources=['classDiagram\n Animal <|-- Dog\n Animal : +name\n Dog : +bark()','stateDiagram-v2\n [*] --> Ready\n Ready --> Done\n Done --> [*]','pie title Allocation\n "A" : 40\n "B" : 60'];
 const {errors}=await fixture(page,baseURL!,info,()=>sources.map(fence));
 await open(page);
 for(let i=0;i<sources.length;i++){
  await page.getByLabel('대화 기록',{exact:true}).dispatchEvent('wheel',{deltaY:-1});
  const block=await loaded(page,i);
  const svg=decodeURIComponent((await block.locator('img').getAttribute('src'))!);
  expect(svg).toContain('<text');expect(svg).not.toContain('<foreignObject');
 }
 expect(errors).toEqual([]);
});

test('manual theme selection redraws Mermaid against the OS preference and preserves source choice',async({page,baseURL},info)=>{
 await page.emulateMedia({colorScheme:'light'});
 const {errors}=await fixture(page,baseURL!,info,()=>[fence(flow)]);await open(page);
 const block=await loaded(page),light=await block.locator('img').getAttribute('src');
 const {sidebarAction}=await import('./navigation');
 await sidebarAction(page,'설정');let dialog=page.getByRole('dialog',{name:'설정',exact:true});
 await dialog.getByLabel('테마',{exact:true}).selectOption('dark');await dialog.getByRole('button',{name:'닫기',exact:true}).click();
 await loaded(page);await expect(block.locator('img')).not.toHaveAttribute('src',light!);
 const dark=await block.locator('img').getAttribute('src');
 await page.emulateMedia({colorScheme:'dark'});await page.emulateMedia({colorScheme:'light'});
 await expect(block.locator('img')).toHaveAttribute('src',dark!);
 await block.getByRole('button',{name:'코드',exact:true}).click();
 await sidebarAction(page,'설정');dialog=page.getByRole('dialog',{name:'설정',exact:true});
 await dialog.getByLabel('테마',{exact:true}).selectOption('light');await dialog.getByRole('button',{name:'닫기',exact:true}).click();
 await expect(block.locator('pre')).toHaveText(flow);
 await block.getByRole('button',{name:'미리보기',exact:true}).click();await loaded(page);
 await expect(block.locator('img')).toHaveAttribute('src',light!);
 expect(errors).toEqual([]);
});
