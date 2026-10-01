import {test,expect} from '@playwright/test';

test('long transcript keeps animated sidebars bounded and old code scrollable',async({page,baseURL},info)=>{
 const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
 await page.route('**/*',async route=>{
  const url=new URL(route.request().url());
  if(url.origin!==new URL(baseURL!).origin||route.request().method()!=='GET'){errors.push('unexpected request');return route.abort();}
  if(url.pathname==='/api/runs/layout-parent'){
   const response=await route.fetch(),data=await response.json();
   const example=data.messages.find((m:{role:string})=>m.role==='assistant');
   data.messages=Array.from({length:600},(_,i)=>({...example,id:'long-'+i,text:`대화 ${i}\n\n${example.text}`,created_at:example.created_at+i}));data.conversation=data.messages;
   return route.fulfill({response,json:data});
  }
  return route.continue();
 });
 await page.emulateMedia({reducedMotion:'no-preference'});await page.setViewportSize({width:1440,height:900});
 await page.goto('/?view=conversation&project=layout-project&run=layout-parent');
 const history=page.getByLabel('대화 기록',{exact:true});await expect(history.locator('.message')).toHaveCount(600);
 await expect.poll(()=>history.evaluate(e=>e.scrollHeight-e.clientHeight-e.scrollTop)).toBeLessThanOrEqual(2);
 const sample=async(label:string)=>{
  const result=await page.evaluate(async name=>{
   let reads=0;const rect=Element.prototype.getBoundingClientRect;
   Element.prototype.getBoundingClientRect=function(){if(this.matches('.markdown pre,.markdown table'))reads++;return rect.call(this);};
   const frames:number[]=[];const widths:number[]=[];
   const button=Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(e=>e.getAttribute('aria-label')===name && e.getClientRects().length)!;
   button.click();const start=performance.now();
   try{await new Promise<void>(resolve=>{const frame=()=>{frames.push(performance.now());widths.push(document.querySelector('.conversation-panel')!.getBoundingClientRect().width);if(performance.now()-start>420)resolve();else requestAnimationFrame(frame);};requestAnimationFrame(frame);});}
   finally{Element.prototype.getBoundingClientRect=rect;}
   return {reads,frames:frames.length,positions:new Set(widths.map(v=>Math.round(v))).size,activeBlocks:document.querySelectorAll('.markdown [data-overlay-scrollbars]').length};
  },label);
  // Regression guard against geometry work growing with all 1,200 code/table owners.
  expect(result.activeBlocks).toBeLessThan(32);expect(result.reads/Math.max(1,result.frames)).toBeLessThan(32);
  // Busy CI machines may deliver only two frames in a 420ms wall-clock sample.
  // Verify interpolation separately using the actual transition's timeline.
  return result;
 };
 const animation=async(label:string)=>{
  const result=await page.evaluate(async name=>{
   const panel=document.querySelector('.conversation-panel')!;
   const before=panel.getBoundingClientRect().width;
   document.documentElement.style.setProperty('--panel-duration','3600s');
   try{
    const button=Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(e=>e.getAttribute('aria-label')===name&&e.getClientRects().length)!;
    button.click();await Promise.resolve();await Promise.resolve();
    panel.getBoundingClientRect();
    const transitions=document.getAnimations().filter(a=>a instanceof CSSTransition&&a.effect instanceof KeyframeEffect&&(a.effect.target as Element)?.matches('.app-shell,.conversation-layout'));
    const positions:number[]=[];
    for(const transition of transitions)transition.pause();
    for(const fraction of [0,.5,1]){
     for(const transition of transitions)transition.currentTime=Number(transition.effect!.getTiming().duration)*fraction;
     positions.push(panel.getBoundingClientRect().width);
    }
    for(const transition of transitions)transition.finish();
    return {before,transitions:transitions.length,positions};
   }finally{document.documentElement.style.removeProperty('--panel-duration');}
  },label);
  expect(result.transitions,'real panel CSS transitions exist').toBeGreaterThan(0);
  const [start,middle,end]=result.positions;
  expect(start).toBeCloseTo(result.before,0);
  expect(Math.abs(end-start)).toBeGreaterThan(10);
  expect(middle).toBeGreaterThan(Math.min(start,end)+1);
  expect(middle).toBeLessThan(Math.max(start,end)-1);
  return result;
 };
 const interpolation=[];
 const initiallyExpanded=await page.locator('.app-shell').evaluate(e=>e.classList.contains('sidebar-expanded'));
 interpolation.push(await animation(initiallyExpanded?'사이드바 닫기':'사이드바 열기'));
 interpolation.push(await animation(initiallyExpanded?'사이드바 열기':'사이드바 닫기'));
 interpolation.push(await animation('업무 맥락'));interpolation.push(await animation('업무 맥락'));
 await info.attach('panel-interpolation',{body:JSON.stringify(interpolation,null,2),contentType:'application/json'});
 const samples=[];
 const expanded=await page.locator('.app-shell').evaluate(e=>e.classList.contains('sidebar-expanded'));
 samples.push(await sample(expanded?'사이드바 닫기':'사이드바 열기'));
 samples.push(await sample(expanded?'사이드바 열기':'사이드바 닫기'));
 samples.push(await sample('업무 맥락'));samples.push(await sample('업무 맥락'));
 await info.attach('panel-layout-work',{body:JSON.stringify(samples,null,2),contentType:'application/json'});
 await expect.poll(()=>history.evaluate(e=>e.scrollHeight-e.clientHeight-e.scrollTop)).toBeLessThanOrEqual(2);
 const historyThumb=page.locator(`.overlay-thumb.vertical[aria-controls="${await history.getAttribute('id')}"]`);
 await historyThumb.focus();await page.keyboard.press('Home');await expect.poll(()=>history.evaluate(e=>e.scrollTop)).toBe(0);
 // Reveal the message before navigating inside it: WebKit defers the layout
 // of descendants in offscreen content-visibility regions.
 const older=history.locator('.message').nth(100);await older.scrollIntoViewIfNeeded();
 const code=older.locator('pre');await expect(code).toHaveAttribute('data-overlay-scrollbars','');
 await code.scrollIntoViewIfNeeded();await expect(code).toBeInViewport();
 await expect(code).toHaveAttribute('data-overlay-scrollbars','');
 const id=await code.getAttribute('id');const thumb=page.locator(`.overlay-thumb.horizontal[aria-controls="${id}"]`);
 await expect(thumb).toBeVisible();await thumb.focus();await page.keyboard.press('End');await expect.poll(()=>code.evaluate(e=>e.scrollLeft)).toBeGreaterThan(0);
 await expect(history.locator('.message').nth(599)).not.toBeInViewport();
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();await expect(code).toBeInViewport();
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();await expect(code).toBeInViewport();
 const draft=page.getByRole('textbox',{name:'메시지',exact:true});await draft.fill('초안 유지');
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();await expect(draft).toHaveValue('초안 유지');
 expect(errors).toEqual([]);
});

// Small upward gestures must release the tail even while still within one line
// of it. Large Home/PageUp jumps alone miss this regression.
for(const input of ['wheel','touch','thumb'] as const)test(`small ${input} scroll reads older messages and resumes following only at the bottom`,async({page,baseURL},info)=>{
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
 await page.setViewportSize({width:input==='touch'?390:1280,height:844});
 let revision=0,emit!:()=>void;
 let update=new Promise<void>(resolve=>emit=resolve);
 const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));
 await page.route('**/*',async route=>{
  const url=new URL(route.request().url());
  if(url.origin!==new URL(baseURL!).origin||route.request().method()!=='GET'){errors.push('unexpected request');return route.abort();}
  if(url.pathname==='/api/stream'){
   if(revision<=Number(url.searchParams.get('after')??0))await update;
   return route.fulfill({contentType:'text/event-stream',body:`event: update\ndata: ${JSON.stringify({seq:revision,kind:'message',data:{run_id:'layout-parent'},id:'scroll-'+revision,created_at:Date.now()})}\n\n`});
  }
  if(url.pathname==='/api/runs/layout-parent'){
   const response=await route.fetch(),data=await response.json();
   const example=data.messages.at(-1);
   for(let i=1;i<=revision;i++)data.messages.push({...example,id:'scroll-update-'+i,text:('새 답변 '+i+'\n\n').repeat(20)});
   data.conversation=data.messages;
   return route.fulfill({response,json:data});
  }
  return route.continue();
 });
 await page.goto('/?view=conversation&project=layout-project&run=layout-parent');
 const history=page.getByLabel('대화 기록',{exact:true});
 const gap=()=>history.evaluate(e=>e.scrollHeight-e.clientHeight-e.scrollTop);
 const settle=()=>page.evaluate(()=>new Promise<void>(resolve=>{let frames=0;const next=()=>{if(++frames===16)resolve();else requestAnimationFrame(next);};requestAnimationFrame(next);}));
 const append=async()=>{
  const send=emit;revision++;update=new Promise<void>(resolve=>emit=resolve);send();
  await expect(history.locator('.message')).toHaveCount(24+revision);
  await settle();
 };
 await expect(history.locator('.message')).toHaveCount(24);
 await expect.poll(gap).toBeLessThanOrEqual(2);
 const thumb=page.getByRole('scrollbar',{name:'대화 기록 세로 스크롤',exact:true});
 if(input==='wheel'){
  await history.hover({position:{x:20,y:100}});await page.mouse.wheel(0,-1);
 }else if(input==='touch'){
  // Native touch scrolling changes scrollTop after touchstart; dispatch that
  // sequence at mobile size without a provider call or CDP-only dependency.
  await history.dispatchEvent('touchstart');
  await history.evaluate(e=>e.scrollBy(0,-1));
  await history.dispatchEvent('touchend');
 }else{await thumb.focus();await page.keyboard.press('ArrowUp');}
 await settle();
 expect(await gap(),'a small upward gesture must not snap back to the tail').toBeGreaterThan(.25);
 const before=await history.evaluate(e=>e.scrollTop);
 await append();
 expect(await history.evaluate(e=>e.scrollTop),'incoming answers must preserve the reading position').toBeCloseTo(before,0);
 expect(await gap()).toBeGreaterThan(100);
 await thumb.focus();await page.keyboard.press('End');await expect.poll(gap).toBeLessThanOrEqual(2);
 await append();await expect.poll(gap).toBeLessThanOrEqual(2);
 expect(errors).toEqual([]);
});
