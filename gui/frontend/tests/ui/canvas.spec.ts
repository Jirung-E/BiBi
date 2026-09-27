import {test,expect,type Page} from '@playwright/test';
const url='/?view=canvas&project=layout-project';
for(const width of [1440,390])test(`work focus and automatic arrangement survive navigation at ${width}px`,async({page})=>{
 await page.setViewportSize({width,height:900});await page.goto(url);
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 const world=page.locator('.world');
 const original=await world.evaluate(e=>getComputedStyle(e).transform);
 await page.locator('[data-work-id="layout-work"]').dblclick({position:{x:10,y:10}});
 await expect(page).toHaveURL(/group=layout-work/);
 await expect(page.locator('.work-group')).toHaveCount(1);
 await expect(page.locator('[data-session-id="layout-other"]')).toHaveCount(0);
 await page.getByRole('button',{name:'자동 정렬',exact:true}).click();
 await page.reload();await expect(page.locator('.work-group')).toHaveCount(1);
 await page.getByRole('button',{name:'전체 그룹',exact:true}).click();
 await expect(page.locator('.work-group')).toHaveCount(2);
 await expect.poll(()=>world.evaluate(e=>getComputedStyle(e).transform)).toBe(original);
 await page.goBack();await expect(page.locator('.work-group')).toHaveCount(1);
 await page.goForward();await expect(page.locator('.work-group')).toHaveCount(2);
 await page.getByRole('button',{name:'자동 정렬',exact:true}).click();
 const boxes=await page.locator('.run-node').evaluateAll(nodes=>nodes.map(n=>n.getBoundingClientRect().toJSON()));
 expect(boxes.length).toBe(5);
 for(const [i,a] of boxes.entries())for(const b of boxes.slice(i+1))expect(a.right<=b.left||b.right<=a.left||a.bottom<=b.top||b.bottom<=a.top).toBe(true);
 expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
});
test('dense repeated transmissions use bounded paths and batch wheel persistence',async({page})=>{
 await page.clock.install();
 await page.addInitScript(()=>{
  const original=Storage.prototype.setItem;
  (window as unknown as {boardWrites:number}).boardWrites=0;
  Storage.prototype.setItem=function(key,value){if(key.startsWith('bibi:board:'))(window as unknown as {boardWrites:number}).boardWrites++;original.call(this,key,value);};
 });
 await page.route('**/api/snapshot',async route=>{
  const response=await route.fetch(),data=await response.json(),edge=data.transmissions[0];
  data.transmissions=Array.from({length:20000},(_,i)=>({...edge,id:'dense-'+i,sent_at:Date.now()-20000+i}));
  await route.fulfill({json:data});
 });
 await page.goto(url);await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await expect(page.locator('.connections>g')).toHaveCount(1);
 // Drain initialization and fit-to-view saves before measuring wheel work.
 // CI can deliver the pointer-up flush before its debounced fit save.
 await page.clock.pauseAt(await page.evaluate(()=>Date.now()+1000));
 await page.clock.runFor(500);
 const writes=await page.evaluate(()=>(window as unknown as {boardWrites:number}).boardWrites);
 expect(writes).toBeGreaterThan(0);
 await page.locator('.board').evaluate(e=>{for(let i=0;i<50;i++)e.dispatchEvent(new WheelEvent('wheel',{deltaY:1,ctrlKey:true,bubbles:true,cancelable:true}));});
 await page.clock.runFor(500);
 expect(await page.evaluate(()=>(window as unknown as {boardWrites:number}).boardWrites)).toBe(writes+1);
 await expect(page.locator('.connections>g')).toHaveCount(1);
});
test('Mac zoom retains its original sensitivity and content has no extra top gap',async({page})=>{
 await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{value:'MacIntel'}));
 await page.goto(url);
 const toolbar=(await page.locator('.content-toolbar').boundingBox())!,board=(await page.locator('.board').boundingBox())!;
 expect(board.y-toolbar.y-toolbar.height).toBeLessThanOrEqual(1);
 const matrix=()=>page.locator('.world').evaluate(e=>new DOMMatrix(getComputedStyle(e).transform).a);
 const before=await matrix();await page.locator('.board').evaluate(e=>e.dispatchEvent(new WheelEvent('wheel',{deltaY:-10,ctrlKey:true,bubbles:true,cancelable:true,clientX:400,clientY:300})));
 await expect.poll(matrix).toBeCloseTo(before*Math.exp(.08),4);
});

async function prepareArrangement(page:Page,width=1440,focus=false,motion:'reduce'|'no-preference'='no-preference'){
 await page.emulateMedia({reducedMotion:motion});await page.setViewportSize({width,height:900});
 await page.clock.install();
 await page.addInitScript(()=>{
  const key='bibi:board:layout-fixture:layout-project';
  if(!localStorage.getItem(key))localStorage.setItem(key,JSON.stringify({points:{
   'layout-parent':{x:40,y:110},'layout-child':{x:650,y:280},'layout-expert':{x:280,y:500},
   'layout-sibling':{x:670,y:550},'layout-other':{x:960,y:700}
  },view:{pan:{x:20,y:40},zoom:.5}}));
  const original=Storage.prototype.setItem;
  (window as unknown as {arrangementWrites:number}).arrangementWrites=0;
  Storage.prototype.setItem=function(key,value){if(key.startsWith('bibi:board:'))(window as unknown as {arrangementWrites:number}).arrangementWrites++;original.call(this,key,value);};
 });
 await page.goto(url+(focus?'&group=layout-work':''));
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await page.clock.pauseAt(await page.evaluate(()=>Date.now()+1000));await page.clock.runFor(500);
 await expect(page.locator('[data-session-id="layout-child"]')).toBeVisible();
}
async function arrangementGeometry(page:Page){
 return page.evaluate(()=>{
  const child=document.querySelector('[data-session-id="layout-child"]') as HTMLElement;
  const scale=parseFloat(child.style.width)/196;
  const nodes=['layout-parent','layout-child','layout-expert','layout-sibling'].map(id=>{
   const node=document.querySelector('[data-session-id="'+id+'"]') as HTMLElement;
   return{x:parseFloat(node.style.left),y:parseFloat(node.style.top),width:parseFloat(node.style.width),height:parseFloat(node.style.height)};
  });
  const group=document.querySelector('[data-work-id="layout-work"]') as HTMLElement;
  const groupBox={x:parseFloat(group.style.left),y:parseFloat(group.style.top),width:parseFloat(group.style.width),height:parseFloat(group.style.height)};
  const edge=document.querySelector('.connections>path') as SVGPathElement;
  const endpoint=edge.getPointAtLength(edge.getTotalLength()).matrixTransform(edge.getScreenCTM()!);
  const box=child.getBoundingClientRect(),matrix=new DOMMatrix(getComputedStyle(document.querySelector('.world')!).transform);
  return{
   x:parseFloat(child.style.left),y:parseFloat(child.style.top),scale,
   edgeError:Math.hypot(endpoint.x-box.left,endpoint.y-(box.top+box.height/2-8*matrix.a)),
   groupError:Math.max(Math.abs(groupBox.x-(Math.min(...nodes.map(n=>n.x))-16*scale)),Math.abs(groupBox.y-(Math.min(...nodes.map(n=>n.y))-38*scale)),Math.abs(groupBox.x+groupBox.width-(Math.max(...nodes.map(n=>n.x+n.width))+16*scale)),Math.abs(groupBox.y+groupBox.height-(Math.max(...nodes.map(n=>n.y+n.height))+16*scale))),
   writes:(window as unknown as {arrangementWrites:number}).arrangementWrites,
   saved:JSON.parse(localStorage.getItem('bibi:board:layout-fixture:layout-project')!).points
  };
 });
}
for(const [width,focus] of [[1440,false],[390,true]] as const)test(`automatic arrangement animates connected geometry and persists only the destination at ${width}px`,async({page})=>{
 await prepareArrangement(page,width,focus);
 const before=await arrangementGeometry(page);
 await page.getByRole('button',{name:'자동 정렬',exact:true}).dispatchEvent('click');
 const frames=[await arrangementGeometry(page)];
 expect(frames[0].x).toBeCloseTo(before.x,4);
 for(let i=0;i<5;i++){await page.clock.runFor(64);frames.push(await arrangementGeometry(page));}
 const final=frames.at(-1)!;
 expect(Math.abs(final.x-before.x)).toBeGreaterThan(50);
 const between=frames.filter(f=>f.x>Math.min(before.x,final.x)+1&&f.x<Math.max(before.x,final.x)-1);
 expect(new Set(between.map(f=>f.x)).size).toBeGreaterThanOrEqual(3);
 for(const frame of frames){expect(frame.edgeError).toBeLessThan(1);expect(frame.groupError).toBeLessThan(1);}
 expect(final.writes).toBe(before.writes+1);
 expect(final.saved['layout-child'].x).toBeCloseTo(final.x/final.scale,4);
 if(focus)expect(final.saved['layout-other']).toEqual(before.saved['layout-other']);
 await page.reload();await expect(page.locator('[data-session-id="layout-child"]')).toBeVisible();
 await expect.poll(async()=>(await arrangementGeometry(page)).x).toBeCloseTo(final.x,4);
});
test('automatic arrangement respects reduced motion',async({page})=>{
 await prepareArrangement(page,1440,false,'reduce');const before=await arrangementGeometry(page);
 await page.getByRole('button',{name:'자동 정렬',exact:true}).dispatchEvent('click');
 const final=await arrangementGeometry(page);expect(Math.abs(final.x-before.x)).toBeGreaterThan(50);
 await page.clock.runFor(80);expect((await arrangementGeometry(page)).x).toBeCloseTo(final.x,4);
 expect(final.edgeError).toBeLessThan(1);expect(final.groupError).toBeLessThan(1);
});
test('manual movement interrupts arrangement at the visible position and another arrangement restarts smoothly',async({page})=>{
 await prepareArrangement(page);
 const arrange=page.getByRole('button',{name:'자동 정렬',exact:true});
 await arrange.dispatchEvent('click');await page.clock.runFor(80);
 const moving=await arrangementGeometry(page);
 await page.locator('[data-work-id="layout-work"]').dispatchEvent('keydown',{key:'ArrowRight'});
 const interrupted=await arrangementGeometry(page);
 expect(interrupted.x).toBeCloseTo(moving.x+20*moving.scale,3);
 await page.clock.runFor(500);expect((await arrangementGeometry(page)).x).toBeCloseTo(interrupted.x,4);
 expect(interrupted.edgeError).toBeLessThan(1);expect(interrupted.groupError).toBeLessThan(1);
 await arrange.dispatchEvent('click');await page.clock.runFor(80);
 const beforeRestart=await arrangementGeometry(page);
 await arrange.dispatchEvent('click');expect((await arrangementGeometry(page)).x).toBeCloseTo(beforeRestart.x,4);
 await page.clock.runFor(500);
 const final=await arrangementGeometry(page);
 expect(Math.abs(final.x-interrupted.x)).toBeGreaterThan(20);
 expect(final.saved['layout-child'].x).toBeCloseTo(final.x/final.scale,4);
});
