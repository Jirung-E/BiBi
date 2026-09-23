import { test as base, expect, type Page } from '@playwright/test';

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
 const width=await page.evaluate(()=>({actual:document.documentElement.scrollWidth,available:document.documentElement.clientWidth}));
 expect(width.actual,'document horizontal overflow').toBeLessThanOrEqual(width.available+1);
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
  await page.getByRole('button',{name:'사용량·연결',exact:true}).click();
  await expect(page.locator('.quota-card')).toHaveCount(1);await noPageOverflow(page);
  await page.getByRole('button',{name:'세션 캔버스',exact:true}).click();
  if(viewport.width<=700)await page.getByRole('button',{name:'상세 닫기',exact:true}).click();
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
  await page.getByRole('button',{name:'설정',exact:true}).click();
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
 await page.getByRole('button',{name:'설정',exact:true}).click();
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
 await page.getByRole('button',{name:'설정',exact:true}).click();
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
 const overflow=await dialog.evaluate(e=>({width:e.scrollWidth-e.clientWidth,children:[...e.querySelectorAll<HTMLElement>('*')].filter(child=>child.clientWidth>0&&child.scrollWidth>child.clientWidth+1).map(child=>({tag:child.tagName,class:child.className,overflow:child.scrollWidth-child.clientWidth,whiteSpace:getComputedStyle(child).whiteSpace}))}));
 expect(overflow.width,JSON.stringify(overflow.children)).toBeLessThanOrEqual(1);
 if(modal==='new')await dialog.getByText('실행 옵션',{exact:true}).click();
 const controls=dialog.locator('input:not([type=hidden]),select,textarea,button');
 for(const control of await controls.all()){
  await control.scrollIntoViewIfNeeded();await expect(control).toBeInViewport();
 }
 await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);
});
