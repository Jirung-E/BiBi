import {test,expect} from '@playwright/test';
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
