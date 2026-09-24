import { test as base, expect, type Page } from '@playwright/test';
import {showSidebar,sidebarAction} from './navigation';

// This production UI can reach only the isolated fixture server. A surprise
// write, external request, or browser exception fails the test.
const test=base.extend<{uiErrors:string[]}>({
 uiErrors:[async({page,baseURL},use)=>{
  const errors:string[]=[];
  page.on('pageerror',error=>errors.push(error.message));
  page.on('response',response=>{if(response.status()>=400)errors.push(`${response.status()} ${response.url()}`);});
  await page.route('**/*',route=>{
   if(new URL(route.request().url()).origin===new URL(baseURL!).origin&&route.request().method()==='GET')return route.continue();
   errors.push(`unexpected request: ${route.request().method()} ${route.request().url()}`);
   return route.abort();
  });
  await use(errors);
  expect(errors).toEqual([]);
 },{auto:true}]
});
const conversation='/?view=conversation&project=layout-project&run=layout-parent';
const sizes=[{width:390,height:844},{width:768,height:1024},{width:1280,height:800},{width:1920,height:1080},{width:1280,height:480}];
async function open(page:Page,scale:number,url=conversation){
 await page.addInitScript(value=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:value,halfLife:30,floor:.15})),scale);
 await page.goto(url);
 await expect(page.locator('.conversation-panel .composer textarea')).toBeVisible();
 await expect(page.locator('.message')).toHaveCount(24);
 await expect.poll(()=>page.evaluate(()=>parseFloat(getComputedStyle(document.documentElement).fontSize))).toBe(14*scale);
}
async function noPageOverflow(page:Page){
 const width=await page.evaluate(()=>({actual:document.documentElement.scrollWidth,available:document.documentElement.clientWidth,
  controls:[...document.querySelectorAll<HTMLElement>('.project-select,.project-controls,.content-toolbar,.conversation-panel,.composer')].map(e=>({class:e.className,width:e.clientWidth,scroll:e.scrollWidth,right:e.getBoundingClientRect().right}))}));
 expect(width.actual,'document horizontal overflow: '+JSON.stringify(width.controls)).toBeLessThanOrEqual(width.available+1);
}
async function contained(page:Page,selector:string,container:string){
 const result=await page.locator(selector).evaluateAll((elements,parentSelector)=>{
  return elements.map(element=>{
   const box=element.getBoundingClientRect(),parent=element.closest(parentSelector)!.getBoundingClientRect();
   return {name:element.className,left:box.left-parent.left,right:parent.right-box.right,top:box.top-parent.top,bottom:parent.bottom-box.bottom};
  });
 },container);
 expect(result.length).toBeGreaterThan(0);
 for(const box of result)for(const edge of ['left','right','top','bottom'] as const)expect(box[edge],`${selector} ${edge}`).toBeGreaterThanOrEqual(-1);
}

for(const viewport of sizes)for(const scale of [.5,1,1.5,2]){
 test(`${viewport.width}×${viewport.height} / UI ${scale*100}%`,async({page})=>{
  await page.setViewportSize(viewport);
  await open(page,scale);
  await noPageOverflow(page);
  await contained(page,'.conversation-panel .composer,.conversation-panel .model-history,.conversation-panel .send','.conversation-panel');
  expect(await page.locator('.model-history button').evaluateAll(buttons=>buttons.filter(e=>e.scrollWidth>e.clientWidth+1).map(e=>e.textContent)),'model labels must wrap inside their buttons').toEqual([]);
  const messages=page.locator('.messages');
  expect(await messages.evaluate(e=>e.scrollHeight>e.clientHeight)).toBe(true);
  await page.getByLabel('메시지',{exact:true}).fill('레이아웃 검증용 초안');
  const send=page.getByRole('button',{name:'전송',exact:true});
  await send.scrollIntoViewIfNeeded();await expect(send).toBeInViewport();await expect(send).toBeEnabled();
  await page.locator('.model-history button').last().scrollIntoViewIfNeeded();
  await expect(page.locator('.model-history button').last()).toBeInViewport();
  await sidebarAction(page,'사용량·연결');
  await expect(page.locator('.quota-card')).toHaveCount(1);await noPageOverflow(page);
  await sidebarAction(page,'세션 캔버스');
  await page.getByRole('button',{name:'상세 닫기',exact:true}).click();
  await page.getByRole('button',{name:'전체 보기',exact:true}).click();
  await expect(page.locator('.run-node')).toHaveCount(5);
  const nodes=await page.locator('.run-node').evaluateAll(elements=>elements.map(e=>({id:e.getAttribute('data-session-id'),...e.getBoundingClientRect().toJSON()})));
  for(const node of nodes){
   const subagent=['layout-child','layout-sibling'].includes(node.id!);
   expect(node.width/node.height,`node proportions: ${node.id}`).toBeCloseTo(subagent?196/84:252/118,2);
  }
  for(let i=0;i<nodes.length;i++)for(let j=i+1;j<nodes.length;j++){
   const a=nodes[i],b=nodes[j],w=Math.min(a.right,b.right)-Math.max(a.left,b.left),h=Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top);
   expect(w<=1||h<=1,`overlapping nodes: ${a.id}, ${b.id}`).toBe(true);
  }
  await contained(page,'.run-node,.work-group','.board');
  const links=await page.locator('.connections>path').evaluateAll(paths=>paths.map((element,index)=>{
   const path=element as SVGPathElement,matrix=path.getScreenCTM()!;
   const start=path.getPointAtLength(0).matrixTransform(matrix),end=path.getPointAtLength(path.getTotalLength()).matrixTransform(matrix);
   const parent=document.querySelector('[data-session-id="layout-parent"]')!.getBoundingClientRect();
   const child=document.querySelector('[data-session-id="'+['layout-child','layout-expert','layout-sibling'][index]+'"]')!.getBoundingClientRect();
   return {start:start.x-parent.right,end:end.x-child.left};
  }));
  expect(links).toHaveLength(3);
  for(const link of links){expect(Math.abs(link.start)).toBeLessThan(1);expect(Math.abs(link.end)).toBeLessThan(1);}
  await noPageOverflow(page);
  await sidebarAction(page,'설정');
  const dialog=page.getByRole('dialog',{name:'설정',exact:true});
  await expect(dialog).toBeVisible();
  const bounds=await dialog.boundingBox();
  expect(bounds!.x).toBeGreaterThanOrEqual(0);expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.x+bounds!.width).toBeLessThanOrEqual(viewport.width+1);
  expect(bounds!.y+bounds!.height).toBeLessThanOrEqual(viewport.height+1);
  expect(await dialog.evaluate(e=>e.scrollWidth-e.clientWidth),'dialog horizontal overflow').toBeLessThanOrEqual(1);
  await dialog.getByRole('button',{name:'닫기',exact:true}).click();await expect(dialog).toHaveCount(0);
 });
}

test('modal wheel scrolling stays inside the dialog and unlocks on Escape',async({page})=>{
 await page.setViewportSize({width:1280,height:480});await open(page,2);
 expect(await page.evaluate(()=>document.documentElement.scrollHeight-innerHeight)).toBeGreaterThan(100);
 await sidebarAction(page,'설정');
 const dialog=page.getByRole('dialog',{name:'설정',exact:true});await expect(dialog).toBeVisible();
 const before=await page.evaluate(()=>scrollY);
 await page.mouse.move(1,240);await page.mouse.wheel(0,750);
 await page.evaluate(()=>new Promise(requestAnimationFrame));await page.evaluate(()=>new Promise(requestAnimationFrame));
 expect(await page.evaluate(()=>scrollY)).toBe(before);
 const bounds=(await dialog.boundingBox())!;
 await page.mouse.move(bounds.x+bounds.width/2,bounds.y+bounds.height/2);await page.mouse.wheel(0,3000);
 await expect.poll(()=>dialog.evaluate(e=>e.scrollTop)).toBeGreaterThan(0);
 await page.mouse.wheel(0,3000);await page.evaluate(()=>new Promise(requestAnimationFrame));
 expect(await page.evaluate(()=>scrollY)).toBe(before);
 await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);
 await page.mouse.move(1,240);await page.mouse.wheel(0,750);
 await expect.poll(()=>page.evaluate(()=>scrollY)).toBeGreaterThan(before);
});

test('UI size persists and moving a work group preserves its children',async({page})=>{
 await page.setViewportSize({width:1280,height:800});
 await page.goto('/?view=canvas&project=layout-project&run=layout-parent');
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 const group=page.getByRole('button',{name:'화면 배율과 긴 대화의 레이아웃 검증 그룹 이동',exact:true});
 await expect(page.locator('.run-node')).toHaveCount(5);
 const positions=():Promise<Record<string,{x:number;y:number}>>=>page.locator('.run-node').evaluateAll(elements=>Object.fromEntries(elements.map(e=>[e.getAttribute('data-session-id'),{x:parseFloat((e as HTMLElement).style.left),y:parseFloat((e as HTMLElement).style.top)}])));
 const before=await positions();await group.press('ArrowRight');const moved=await positions();
 for(const id of ['layout-parent','layout-child','layout-expert','layout-sibling'])expect(moved[id].x-before[id].x).toBeCloseTo(20);
 expect(moved['layout-other']).toEqual(before['layout-other']);
 await sidebarAction(page,'설정');
 await page.getByRole('slider',{name:/UI 크기/}).press('End');
 await expect(page.getByRole('slider',{name:'UI 크기 · 200%'})).toHaveValue('2');
 await page.getByRole('button',{name:'닫기',exact:true}).click();
 await page.reload();await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await expect(page.locator('.run-node')).toHaveCount(5);
 const scaled=await positions();
 for(const [id,point] of Object.entries(moved)){expect(scaled[id].x).toBeCloseTo(point.x*2);expect(scaled[id].y).toBeCloseTo(point.y*2);}
});

for(const modal of ['project','new','context','host','settings'])test(`mobile enlarged ${modal} form remains reachable`,async({page})=>{
 await page.setViewportSize({width:390,height:844});await open(page,2,conversation+'&modal='+modal);
 const dialog=page.locator('dialog');await expect(dialog).toBeVisible();
 if(modal==='settings')await dialog.getByRole('button',{name:'+ 제공자 추가',exact:true}).click();
 const overflow=await dialog.evaluate(e=>({width:e.scrollWidth-e.clientWidth,children:[...e.querySelectorAll<HTMLElement>('*')].filter(child=>child.clientWidth>0&&child.scrollWidth>child.clientWidth+1).map(child=>({tag:child.tagName,class:child.className,overflow:child.scrollWidth-child.clientWidth,whiteSpace:getComputedStyle(child).whiteSpace,width:child.getBoundingClientRect().width,controls:[...child.children].map(control=>({tag:control.tagName,width:control.getBoundingClientRect().width}))}))}));
 expect(overflow.width,JSON.stringify(overflow.children)).toBeLessThanOrEqual(1);
 if(modal==='new')await dialog.getByText('실행 옵션',{exact:true}).click();
 const controls=dialog.locator('input:not([type=hidden]),select,textarea,button');
 for(const control of await controls.all()){
  await control.scrollIntoViewIfNeeded();await expect(control).toBeInViewport();
 }
 await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);
});


test('canvas uses the available window and session controls stay within reach',async({page})=>{
 await page.setViewportSize({width:1280,height:800});await open(page,1);
 await sidebarAction(page,'세션 캔버스');
 const board=page.locator('.board');
 const selected=(await board.boundingBox())!;
 expect(selected.height).toBeGreaterThan(520);
 expect(selected.y+selected.height).toBeLessThan(800);
 expect(await page.evaluate(()=>document.documentElement.scrollHeight-innerHeight)).toBeLessThanOrEqual(1);
 await expect(page.getByRole('button',{name:'대화 열기 ↗',exact:true})).toBeInViewport();
 await page.getByRole('button',{name:'상세 닫기',exact:true}).click();
 await expect(page.locator('.run-details')).toHaveCount(0);
 expect((await board.boundingBox())!.width).toBeGreaterThan(selected.width+150);
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await page.locator('[data-session-id="layout-parent"]').click();
 await page.getByRole('button',{name:'대화 열기 ↗',exact:true}).click();
 await expect(page.locator('.conversation-panel')).toBeVisible();
 for(const view of ['작업 대화','사용량·연결','세션 캔버스']){
  await sidebarAction(page,view);
  await showSidebar(page);
  const projectPicker=page.getByLabel('프로젝트',{exact:true});
  await projectPicker.focus();
  expect(await projectPicker.evaluate(e=>getComputedStyle(e.parentElement!).outlineStyle)).toBe('solid');
  const picker=(await projectPicker.boundingBox())!;
  const add=page.getByRole('button',{name:'프로젝트 추가',exact:true});
  const bounds=(await add.boundingBox())!;
  expect(bounds.x-picker.x-picker.width).toBeGreaterThanOrEqual(0);
  expect(bounds.x-picker.x-picker.width).toBeLessThan(16);
  expect(Math.abs(bounds.y+bounds.height/2-picker.y-picker.height/2)).toBeLessThan(2);
  await add.click();await expect(page.getByRole('dialog',{name:'프로젝트 추가',exact:true})).toBeVisible();
  await page.goBack();await expect(page.locator('dialog')).toHaveCount(0);
  await expect(page.locator('main')).toHaveClass(new RegExp(view==='세션 캔버스'?'canvas':view==='작업 대화'?'conversation':'usage'));
 }
 await page.locator('.quota-pill').click();await expect(page.locator('.quota-card')).toHaveCount(1);
});

test('many providers scroll within the sidebar and discovery belongs to the canvas',async({page})=>{
 await page.route('**/api/snapshot',async route=>{
  const response=await route.fetch();const data=await response.json();
  const provider=data.providers[0],quota=data.quotas[0];
  data.providers=Array.from({length:12},(_,i)=>({...provider,id:'layout-provider-'+i,adapter:i===0?'codex':'mock'}));
  data.quotas=data.providers.map((p:{id:string},i:number)=>({...quota,id:'quota-'+i,provider_id:p.id,status:i%3===0?'unknown':i%3===1?'error':'unlimited',windows:[]}));
  await route.fulfill({response,json:data});
 });
 await page.setViewportSize({width:1280,height:800});await open(page,1);
 await sidebarAction(page,'세션 캔버스');
 const strip=page.locator('.quota-strip');await expect(strip.locator('button')).toHaveCount(12);
 expect(await page.locator('.sidebar-scroll').evaluate(e=>e.scrollHeight>e.clientHeight)).toBe(true);
 expect(await strip.evaluate(e=>e.closest('.app-sidebar')!==null)).toBe(true);
 expect((await page.locator('.board').boundingBox())!.height).toBeGreaterThan(520);
 await noPageOverflow(page);
 await page.getByText('외부 세션 찾기',{exact:true}).click();
 await expect(page.locator('.session-import-menu button')).toHaveCount(1);
 await sidebarAction(page,'사용량·연결');
 await expect(page.locator('.quota-card')).toHaveCount(12);
 await expect(page.getByText('외부 세션 찾기',{exact:true})).toHaveCount(0);
 await expect(page.getByRole('button',{name:'호스트 연결',exact:true})).toBeVisible();
});

async function camera(page:Page){
 return page.locator('.board').evaluate(board=>{
  const matrix=new DOMMatrix(getComputedStyle(board.querySelector('.world')!).transform);
  return {width:board.clientWidth,height:board.clientHeight,zoom:matrix.a,x:(board.clientWidth/2-matrix.e)/matrix.a,y:(board.clientHeight/2-matrix.f)/matrix.a};
 });
}

for(const viewport of [{width:1440,height:900},{width:390,height:844}])test('inspector preserves center and restores size at '+viewport.width+'px',async({page})=>{
 await page.setViewportSize(viewport);await page.goto('/?view=canvas&project=layout-project');
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 const original=await camera(page);
 const target=page.locator('[data-session-id="layout-child"]');
 for(let repeat=0;repeat<3;repeat++){
  await target.click();
  await expect(page.locator('.run-details')).toBeVisible();
  await expect.poll(async()=>(await camera(page)).zoom).toBeLessThan(original.zoom);
  const shrunk=await camera(page);
  expect(shrunk.x).toBeCloseTo(original.x,1);expect(shrunk.y).toBeCloseTo(original.y,1);
  await contained(page,'.run-node','.board');
  const board=(await page.locator('.board').boundingBox())!,panel=(await page.locator('.run-details').boundingBox())!;
  expect(board.x+board.width<=panel.x+1||board.y+board.height<=panel.y+1,'inspector must never overlap the board').toBe(true);
  await page.getByRole('button',{name:'상세 닫기',exact:true}).click();
  await expect.poll(async()=>(await camera(page)).zoom).toBeCloseTo(original.zoom,5);
 }
 await showSidebar(page);
 if(viewport.width>1000){
  await page.getByRole('button',{name:'사이드바 닫기',exact:true}).click();
  const expanded=await camera(page);
  expect(expanded.width).toBeGreaterThan(original.width);expect(expanded.x).toBeCloseTo(original.x,1);
  await page.reload();
  await expect(page.locator('.app-shell')).not.toHaveClass(/sidebar-expanded/);
  await expect(page.locator('.world')).toBeAttached();
  expect((await camera(page)).x).toBeCloseTo(original.x,1);
 }else{
  const drawer=page.getByRole('dialog',{name:'사이드바',exact:true});
  await expect(drawer).toBeVisible();
  await expect.poll(()=>drawer.evaluate(e=>e.contains(document.activeElement))).toBe(true);
  await page.keyboard.press('Escape');await expect(drawer).toHaveCount(0);
  await expect(page.getByRole('button',{name:'사이드바 열기',exact:true})).toBeFocused();
 }
});

test('animated inspector resizes continuously with shared node and edge geometry',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});
 await page.setViewportSize({width:1440,height:900});
 await page.goto('/?view=canvas&project=layout-project');
 await expect.poll(()=>page.locator('.app-sidebar').evaluate(e=>e.clientWidth)).toBe(216);
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 const original=await camera(page);
 // Capture layout frames; opening and closing still use real pointer input.
 await page.evaluate(()=>{
  const samples:{width:number;zoom:number;edgeError:number}[]=[];
  (window as unknown as {layoutFrames:typeof samples}).layoutFrames=samples;
  const sample=()=>{
   const board=document.querySelector('.board')!,world=document.querySelector('.world')!;
   const matrix=new DOMMatrix(getComputedStyle(world).transform),path=document.querySelector('.connections>path') as SVGPathElement;
   const end=path.getPointAtLength(path.getTotalLength()).matrixTransform(path.getScreenCTM()!);
   samples.push({width:board.clientWidth,zoom:matrix.a,edgeError:end.x-document.querySelector('[data-session-id="layout-child"]')!.getBoundingClientRect().left});
   if(samples.length<120)requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
 });
 await page.locator('[data-session-id="layout-child"]').click();
 await expect.poll(async()=>(await camera(page)).width).toBe(original.width-280);
 await page.getByRole('button',{name:'상세 닫기',exact:true}).click();
 await expect.poll(async()=>(await camera(page)).width).toBe(original.width);
 const samples=await page.evaluate(()=>(window as unknown as {layoutFrames:{width:number;zoom:number;edgeError:number}[]}).layoutFrames);
 expect(new Set(samples.filter(s=>s.width<original.width-2&&s.width>original.width-278).map(s=>s.width)).size).toBeGreaterThan(3);
 for(const sample of samples)expect(Math.abs(sample.edgeError)).toBeLessThan(1);
 const final=await camera(page);
 expect(final.zoom).toBeCloseTo(original.zoom,5);expect(final.x).toBeCloseTo(original.x,1);
});

test('conversation shares the sidebar session list and opens context only when requested',async({page})=>{
 await page.setViewportSize({width:1440,height:900});await open(page,1);
 await expect(page.locator('.conversation-layout .history')).toHaveCount(0);
 await expect(page.locator('.app-sidebar .history button')).toHaveCount(4);
 await expect(page.locator('.context-panel')).toHaveCount(0);
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 await expect(page.locator('.context-panel')).toBeVisible();
 await page.getByRole('button',{name:'맥락 닫기',exact:true}).click();
 await expect(page.locator('.context-panel')).toHaveCount(0);
});
