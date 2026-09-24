import { test as base, expect, type Page } from '@playwright/test';
import {showSidebar,sidebarAction} from './navigation';
import {expectOverlayScrolling} from './scrolling';

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
 const base=await page.evaluate(()=>navigator.platform.startsWith('Win')?16:14);
 await expect.poll(()=>page.evaluate(()=>parseFloat(getComputedStyle(document.documentElement).fontSize))).toBe(base*scale);
}
async function noPageOverflow(page:Page){
 const width=await page.evaluate(()=>({actual:document.documentElement.scrollWidth,available:document.documentElement.clientWidth,
  controls:[...document.querySelectorAll<HTMLElement>('.project-select,.project-controls,.content-toolbar,.conversation-panel,.composer')].map(e=>({class:e.className,width:e.clientWidth,scroll:e.scrollWidth,right:e.getBoundingClientRect().right}))}));
 expect(width.actual,'document horizontal overflow: '+JSON.stringify(width.controls)).toBeLessThanOrEqual(width.available+1);
 const vertical=await page.evaluate(()=>({height:document.documentElement.scrollHeight,viewport:innerHeight,y:scrollY}));
 expect(vertical.height,'document must not scroll vertically').toBeLessThanOrEqual(vertical.viewport+1);
 expect(vertical.y).toBe(0);
 expect(await page.locator('main').evaluate(e=>getComputedStyle(e).scrollbarWidth),'navigation must retain overlay scrolling').toBe('none');
 await expectOverlayScrolling(page);
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
  await contained(page,'.conversation-panel .composer,.messages,.conversation-heading','.conversation-panel');
  const messages=page.locator('.messages');
  expect(await messages.evaluate(e=>e.scrollHeight>e.clientHeight)).toBe(true);
  const panel=(await page.locator('.conversation-panel').boundingBox())!,composer=(await page.locator('.conversation-panel>.composer').boundingBox())!;
  expect(composer.height,'input area must remain usable without growing the page').toBeGreaterThan(Math.min(100*scale,panel.height*.4));
  expect((await messages.boundingBox())!.height,'history keeps its own visible viewport').toBeGreaterThan(panel.height*.12);
  await page.getByLabel('메시지',{exact:true}).fill('레이아웃 검증용 초안');
  const send=page.getByRole('button',{name:'전송',exact:true});
  await send.scrollIntoViewIfNeeded();await expect(send).toBeInViewport();await expect(send).toBeEnabled();
  await page.getByRole('button',{name:'최근 모델',exact:true}).click();
  expect(await page.locator('.model-history button').evaluateAll(buttons=>buttons.filter(e=>e.scrollWidth>e.clientWidth+1).map(e=>e.textContent)),'model labels must wrap inside their buttons').toEqual([]);
  await page.locator('.model-history button').last().scrollIntoViewIfNeeded();
  await expect(page.locator('.model-history button').last()).toBeInViewport();
  await noPageOverflow(page);
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
  const dock=(await page.locator('.board-tools').boundingBox())!;
  for(const node of nodes)expect(node.bottom<=dock.y||node.right<=dock.x,'fit must keep session nodes clear of floating controls').toBe(true);
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
 await sidebarAction(page,'사용량·연결');
 const owner=page.locator('main.usage');
 expect(await owner.evaluate(e=>e.scrollHeight-e.clientHeight)).toBeGreaterThan(100);
 await sidebarAction(page,'설정');
 const dialog=page.getByRole('dialog',{name:'설정',exact:true});await expect(dialog).toBeVisible();
 const before=await owner.evaluate(e=>e.scrollTop);
 await page.mouse.move(1,240);await page.mouse.wheel(0,750);
 await page.evaluate(()=>new Promise(requestAnimationFrame));await page.evaluate(()=>new Promise(requestAnimationFrame));
 expect(await owner.evaluate(e=>e.scrollTop)).toBe(before);
 await noPageOverflow(page);
 const bounds=(await dialog.boundingBox())!;
 await page.mouse.move(bounds.x+bounds.width/2,bounds.y+bounds.height/2);await page.mouse.wheel(0,3000);
 await expect.poll(()=>dialog.evaluate(e=>e.scrollTop)).toBeGreaterThan(0);
 await page.mouse.wheel(0,3000);await page.evaluate(()=>new Promise(requestAnimationFrame));
 expect(await owner.evaluate(e=>e.scrollTop)).toBe(before);
 await noPageOverflow(page);
 await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);
 const area=(await owner.boundingBox())!;await page.mouse.move(area.x+area.width/2,area.y+area.height/2);await page.mouse.wheel(0,750);
 await expect.poll(()=>owner.evaluate(e=>e.scrollTop)).toBeGreaterThan(before);
 await noPageOverflow(page);
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
 if(modal==='settings')await dialog.getByRole('button',{name:'제공자 추가',exact:true}).click();
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
 await expect(page.getByRole('button',{name:'대화 열기',exact:true})).toBeInViewport();
 await page.getByRole('button',{name:'상세 닫기',exact:true}).click();
 await expect(page.locator('.run-details')).toHaveCount(0);
 expect((await board.boundingBox())!.width).toBeGreaterThan(selected.width+150);
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await page.locator('[data-session-id="layout-parent"]').click();
 await page.getByRole('button',{name:'대화 열기',exact:true}).click();
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
  data.providers=Array.from({length:12},(_,i)=>({...provider,id:'layout-provider-'+i,adapter:i<8?'codex':'mock'}));
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
 await expect(page.locator('.session-import-menu button')).toHaveCount(8);
 const menu=page.locator('.session-import-menu');
 expect(await menu.evaluate(e=>e.scrollHeight>e.clientHeight)).toBe(true);
 await menu.locator('button').last().scrollIntoViewIfNeeded();
 await expect(menu.locator('button').last()).toBeInViewport();
 await expectOverlayScrolling(page);
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
  await expect.poll(async()=>(await camera(page)).x).toBeCloseTo(original.x,1);
  expect((await camera(page)).width).toBeGreaterThan(original.width);
  await page.reload();
  await expect(page.locator('.app-shell')).not.toHaveClass(/sidebar-expanded/);
  await expect(page.locator('.world')).toBeAttached();
  await expect.poll(async()=>(await camera(page)).x).toBeCloseTo(original.x,1);
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
 const font=await page.evaluate(()=>parseFloat(getComputedStyle(document.documentElement).fontSize));
 await expect.poll(()=>page.locator('.sidebar-slot').evaluate(e=>e.clientWidth)).toBe(18*font);
 const inspectorWidth=22*font;
 expect(await page.locator('.canvas-layout').evaluate(e=>getComputedStyle(e).transitionDuration.split(',').map(parseFloat))).toEqual([.26,.26]);
 // Keep the production duration checked above, but give low-FPS CI WebKit
 // enough time to sample the same transition geometry deterministically.
 await page.addStyleTag({content:'.app-shell { --panel-duration: 800ms; }'});
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
   if(samples.length<300)requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
 });
 await page.locator('[data-session-id="layout-child"]').click();
 await expect.poll(async()=>(await camera(page)).width).toBe(original.width-inspectorWidth);
 await page.getByRole('button',{name:'상세 닫기',exact:true}).click();
 await expect.poll(async()=>(await camera(page)).width).toBe(original.width);
 const samples=await page.evaluate(()=>(window as unknown as {layoutFrames:{width:number;zoom:number;edgeError:number}[]}).layoutFrames);
 expect(new Set(samples.filter(s=>s.width<original.width-2&&s.width>original.width-inspectorWidth+2).map(s=>s.width)).size).toBeGreaterThan(3);
 for(const sample of samples)expect(Math.abs(sample.edgeError)).toBeLessThan(1);
 const final=await camera(page);
 expect(final.zoom).toBeCloseTo(original.zoom,5);expect(final.x).toBeCloseTo(original.x,1);
});

test('conversation shares the sidebar session list and opens context only when requested',async({page})=>{
 await page.setViewportSize({width:1440,height:900});await open(page,1);
 const title=(await page.locator('.conversation-heading>div').first().boundingBox())!;
 const actions=(await page.locator('.conversation-heading>.session-actions').boundingBox())!;
 expect(actions.x).toBeGreaterThanOrEqual(title.x+title.width);
 await expect(page.locator('.conversation-layout .history')).toHaveCount(0);
 await expect(page.locator('.app-sidebar .history button')).toHaveCount(4);
 await expect(page.locator('.context-panel')).toBeHidden();
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 await expect(page.locator('.context-panel')).toBeVisible();
 await page.getByRole('button',{name:'맥락 닫기',exact:true}).click();
 await expect(page.locator('.context-panel')).toBeHidden();
});

// Exercise the overlay's geometry at both UI scale extremes without starting
// a desktop process or calling a model. IPC reads use the same closed fixture.
for(const scale of [.5,1,2])test('native Mac titlebar clearance / UI '+scale*100+'%',async({page})=>{
 await page.setViewportSize({width:1280,height:800});
 await page.addInitScript(value=>{
  localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:value}));
  Object.defineProperty(navigator,'platform',{value:'MacIntel'});
  Object.defineProperty(window,'__TAURI_INTERNALS__',{value:{
   async invoke(cmd:string,args:{path?:string;method?:string}={}){
    if(cmd==='connection_info')return {mode:'local',url:location.origin};
    if(cmd==='api_request'&&args.method==='GET'){
     if(args.path?.startsWith('/api/events?'))return [];
     return (await fetch(args.path!)).json();
    }
    throw new Error('Unexpected native command: '+cmd);
   }
  }});
 },scale);
 await page.goto('/?view=canvas&project=layout-project');
 await expect(page.locator('.app-shell')).toHaveClass(/mac-window/);
 await expect.poll(()=>page.evaluate(()=>parseFloat(getComputedStyle(document.documentElement).fontSize))).toBe(14*scale);
 // Large UI starts with offscreen nodes culled; fitting makes all five visible.
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await expect(page.locator('.run-node')).toHaveCount(5);
 async function clearNativeControls(){
  const toolbar=(await page.locator('.content-toolbar').boundingBox())!;
  expect(toolbar.height).toBeGreaterThanOrEqual(56);
  const toggle=page.getByRole('button',{name:scale===2?'사이드바 열기':'사이드바 닫기',exact:true});
  expect((await toggle.boundingBox())!.x).toBeGreaterThanOrEqual(96);
  await noPageOverflow(page);
 }
 await clearNativeControls();
 if(scale!==2){
  await page.getByRole('button',{name:'사이드바 닫기',exact:true}).click();
  await expect(page.locator('.app-shell')).not.toHaveClass(/sidebar-expanded/);
  expect((await page.getByRole('button',{name:'사이드바 열기',exact:true}).boundingBox())!.x).toBeGreaterThanOrEqual(96);
 }
 await showSidebar(page);
 if(scale===2)expect((await page.getByRole('dialog',{name:'사이드바',exact:true}).boundingBox())!.y).toBeGreaterThanOrEqual(56);
 await page.getByRole('button',{name:'프로젝트 추가',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'프로젝트 추가',exact:true});
 await expect(dialog).toBeVisible();
 expect((await dialog.boundingBox())!.y).toBeGreaterThanOrEqual(56);
 await page.keyboard.press('Escape');
 await expect(dialog).toHaveCount(0);
 await page.getByRole('button',{name:'새 업무',exact:true}).click();
 await expect(page.getByRole('dialog',{name:'새 업무',exact:true})).toBeVisible();
});

for(const scale of [.5,1,2])test('Windows custom titlebar controls / UI '+scale*100+'%',async({page})=>{
 await page.setViewportSize({width:640,height:480});
 await page.addInitScript(value=>{
  localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:value}));
  Object.defineProperty(navigator,'platform',{value:'Win32'});
  const calls:string[]=[];let maximized=false;
  Object.defineProperty(window,'nativeCalls',{value:calls});
  Object.defineProperty(window,'__TAURI_INTERNALS__',{value:{
   metadata:{currentWindow:{label:'main'}},
   async invoke(cmd:string,args:{path?:string;method?:string;label?:string}={}){
    if(cmd==='connection_info')return {mode:'local',url:location.origin};
    if(cmd==='api_request'&&args.method==='GET'){
     if(args.path?.startsWith('/api/events?'))return [];
     return (await fetch(args.path!)).json();
    }
    if(cmd==='plugin:window|is_maximized')return maximized;
    if(['plugin:window|minimize','plugin:window|toggle_maximize','plugin:window|close'].includes(cmd)&&args.label==='main'){
     calls.push(cmd);if(cmd.endsWith('toggle_maximize'))maximized=!maximized;return;
    }
    throw new Error('Unexpected native command: '+cmd);
   }
  }});
 },scale);
 await page.goto('/?view=canvas&project=layout-project');
 await expect(page.locator('.app-shell')).toHaveClass(/windows-window/);
 const controls=page.locator('.window-controls');await expect(controls.locator('button')).toHaveCount(3);
 const bounds=(await controls.boundingBox())!;expect(bounds.x+bounds.width).toBe(640);expect(bounds.y).toBe(0);expect(bounds.width).toBe(138);
 const actions=(await page.locator('.toolbar-actions').boundingBox())!;
 expect(actions.x+actions.width).toBeLessThanOrEqual(bounds.x);
 await expect(page.locator('.toolbar-title')).toHaveAttribute('data-tauri-drag-region','');
 await expect(page.getByRole('button',{name:'새 업무',exact:true})).not.toHaveAttribute('data-tauri-drag-region','');
 await page.getByRole('button',{name:'창 최소화',exact:true}).click();
 await page.getByRole('button',{name:'창 최대화',exact:true}).click();
 await page.getByRole('button',{name:'이전 창 크기로',exact:true}).click();
 await page.getByRole('button',{name:'창 닫기',exact:true}).click();
 expect(await page.evaluate(()=>(window as unknown as {nativeCalls:string[]}).nativeCalls)).toEqual(['plugin:window|minimize','plugin:window|toggle_maximize','plugin:window|toggle_maximize','plugin:window|close']);
 await noPageOverflow(page);
 await showSidebar(page);
 const drawer=page.getByRole('dialog',{name:'사이드바',exact:true});
 if(await drawer.count()){
  const chrome=(await drawer.locator('.window-controls').boundingBox())!;
  expect(chrome.y).toBe(0);expect(chrome.x+chrome.width).toBe(640);
  await drawer.getByRole('button',{name:'창 최소화',exact:true}).click();
 }
 await sidebarAction(page,'설정');
 const dialog=page.getByRole('dialog',{name:'설정',exact:true});
 await expect(dialog).toBeVisible();
 const buttons=dialog.locator('.window-controls'),dialogBounds=(await dialog.boundingBox())!,buttonBounds=(await buttons.boundingBox())!;
 expect(buttonBounds.y).toBe(0);expect(buttonBounds.x+buttonBounds.width).toBe(640);expect(dialogBounds.y).toBeGreaterThanOrEqual(56);
 await dialog.getByRole('button',{name:'창 최소화',exact:true}).click();
 expect(await page.evaluate(()=>(window as unknown as {nativeCalls:string[]}).nativeCalls.at(-1))).toBe('plugin:window|minimize');
});

test('overlay scrollbars reserve no space and support pointer, keyboard and dialog input',async({page})=>{
 await page.setViewportSize({width:1280,height:800});await open(page,1);
 await expect(page.locator('.window-controls')).toHaveCount(0);
 const messages=page.locator('.messages'),id=await messages.getAttribute('id');
 const thumb=page.locator('.overlay-thumb.vertical[aria-controls="'+id+'"]');
 await expect(thumb).toBeAttached();await thumb.focus();await page.keyboard.press('Home');
 await expect.poll(()=>messages.evaluate(e=>e.scrollTop)).toBe(0);
 // Native scrolling updates before the overlay's animation-frame geometry.
 await expect(thumb).toHaveAttribute('aria-valuenow','0');
 const before=(await messages.boundingBox())!;
 const gutter=await messages.evaluate(e=>{const s=getComputedStyle(e);return (e as HTMLElement).offsetWidth-e.clientWidth-parseFloat(s.borderLeftWidth)-parseFloat(s.borderRightWidth);});
 expect(gutter).toBe(0);
 const box=(await thumb.boundingBox())!;
 expect(box.x).toBeGreaterThan(before.x+before.width-16);
 await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();
 await page.mouse.move(box.x+box.width/2,before.y+before.height-5,{steps:8});await page.mouse.up();
 await expect.poll(()=>messages.evaluate(e=>e.scrollTop)).toBeGreaterThan(100);
 await thumb.focus();await page.keyboard.press('End');
 await expect.poll(()=>messages.evaluate(e=>e.scrollHeight-e.clientHeight-e.scrollTop)).toBeLessThanOrEqual(1);
 expect((await messages.boundingBox())!.width).toBe(before.width);
 await noPageOverflow(page);
 const textarea=page.getByRole('textbox',{name:'메시지',exact:true});
 await textarea.fill(Array.from({length:30},(_,i)=>'입력 '+i).join('\n'));
 const inputThumb=page.locator('.overlay-thumb.vertical[aria-controls="'+await textarea.getAttribute('id')+'"]');
 await expect(inputThumb).toBeVisible();await inputThumb.focus();await page.keyboard.press('End');
 await expect.poll(()=>textarea.evaluate(e=>e.scrollTop)).toBeGreaterThan(0);
 await sidebarAction(page,'설정');
 const dialog=page.getByRole('dialog',{name:'설정',exact:true});
 await dialog.getByRole('button',{name:'제공자 추가',exact:true}).click();
 await dialog.getByRole('button',{name:'Ollama',exact:true}).click();
 const modalId=await dialog.getAttribute('id'),modalThumb=dialog.locator('.overlay-thumb.vertical[aria-controls="'+modalId+'"]');
 await expect(modalThumb).toBeVisible();
 const modalBox=(await dialog.boundingBox())!,barBox=(await modalThumb.boundingBox())!;
 expect(barBox.x).toBeGreaterThan(modalBox.x);expect(barBox.x+barBox.width).toBeLessThanOrEqual(modalBox.x+modalBox.width);
 expect(barBox.y).toBeGreaterThanOrEqual(modalBox.y);expect(barBox.y+barBox.height).toBeLessThanOrEqual(modalBox.y+modalBox.height);
 await modalThumb.focus();await page.keyboard.press('End');
 await expect.poll(()=>dialog.evaluate(e=>e.scrollTop)).toBeGreaterThan(0);
 await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);
 await expect(page.locator('body>.scrollbar-layer')).not.toHaveCount(0);
 await noPageOverflow(page);
});

for(const viewport of [{width:390,height:844},{width:1280,height:800},{width:1280,height:480}])test(`chat reserves most space for messages at ${viewport.width}×${viewport.height}`,async({page})=>{
 await page.setViewportSize(viewport);await open(page,1);
 const panel=(await page.locator('.conversation-panel').boundingBox())!,messages=(await page.locator('.messages').boundingBox())!,composer=(await page.locator('.composer').boundingBox())!;
 expect(messages.height/panel.height).toBeGreaterThan(.53);
 expect(composer.height).toBeLessThan(155);
 expect(panel.y+panel.height-composer.y-composer.height).toBeLessThan(2);
 expect(viewport.height-panel.y-panel.height).toBeLessThanOrEqual(8);
 await expect(page.getByLabel('모델',{exact:true})).toBeEnabled();
 await expect(page.locator('.model-history')).toHaveCount(0);
 await expect(page.getByRole('combobox',{name:'전송 방식',exact:true})).toHaveCount(0);
 await page.getByRole('button',{name:'전송 설정',exact:true}).click();
 await expect(page.getByRole('combobox',{name:'전송 방식',exact:true})).toHaveValue('continue');
 await noPageOverflow(page);
});
test('Ctrl wheel limits a Windows notch and keeps the pointer anchored',async({page})=>{
 await page.setViewportSize({width:1280,height:800});await page.goto('/?view=canvas&project=layout-project');
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 const board=page.locator('.board'),node=page.locator('.run-node').first();
 const box=(await board.boundingBox())!,before=(await node.boundingBox())!,pointer={x:box.x+box.width*.6,y:box.y+box.height*.4};
 await page.mouse.move(pointer.x,pointer.y);await page.keyboard.down('Control');
 try{await page.mouse.wheel(0,-120);}finally{await page.keyboard.up('Control');}
 await expect.poll(async()=>(await node.boundingBox())!.width).toBeGreaterThan(before.width);
 const after=(await node.boundingBox())!,ratio=after.width/before.width;
 expect(ratio).toBeLessThan(1.14);expect(ratio).toBeGreaterThan(1.05);
 expect(after.x).toBeCloseTo(pointer.x+(before.x-pointer.x)*ratio,0);
 expect(after.y).toBeCloseTo(pointer.y+(before.y-pointer.y)*ratio,0);
 await noPageOverflow(page);
});
