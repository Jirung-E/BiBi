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
  expect(result.positions,'panel still animates through intermediate positions').toBeGreaterThan(2);
  return result;
 };
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
