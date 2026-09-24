import {test as base,expect,type Page,type Locator} from '@playwright/test';
import {sidebarAction} from './navigation';

const test=base.extend<{uiErrors:string[]}>({
 uiErrors:[async({page,baseURL},use)=>{
  const errors:string[]=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',route=>{
   if(new URL(route.request().url()).origin===new URL(baseURL!).origin&&route.request().method()==='GET')return route.continue();
   errors.push('unexpected request '+route.request().url());return route.abort();
  });
  await use(errors);expect(errors).toEqual([]);
 },{auto:true}]
});
const chat='/?view=conversation&project=layout-project&run=layout-parent';
const board='/?view=canvas&project=layout-project';
async function font(page:Page){
 const base=await page.evaluate(()=>navigator.platform.startsWith('Win')?16:14);
 await expect.poll(()=>page.evaluate(()=>parseFloat(getComputedStyle(document.documentElement).fontSize))).toBe(base);
 return base;
}
async function drag(page:Page,handle:Locator,x:number,y=0){
 const box=(await handle.boundingBox())!;
 await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();
 await page.mouse.move(box.x+box.width/2+x,box.y+box.height/2+y,{steps:8});await page.mouse.up();
}
async function bounded(page:Page){
 expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
 expect(await page.evaluate(()=>document.documentElement.scrollHeight-innerHeight)).toBeLessThanOrEqual(1);
}
async function camera(page:Page){
 return page.locator('.board').evaluate(e=>{
  const m=new DOMMatrix(getComputedStyle(e.querySelector('.world')!).transform);
  return {x:(e.clientWidth/2-m.e)/m.a,y:(e.clientHeight/2-m.f)/m.a,width:e.clientWidth,zoom:m.a};
 });
}

test('inset sidebar resizes, clamps, remembers its width and fully collapses',async({page})=>{
 await page.setViewportSize({width:1440,height:900});await page.goto(board);
 const f=await font(page),sidebar=page.locator('.app-sidebar'),slot=page.locator('.sidebar-slot');
 const shape=await sidebar.evaluate(e=>({box:e.getBoundingClientRect().toJSON(),radius:parseFloat(getComputedStyle(e).borderTopLeftRadius)}));
 expect(shape.box.x).toBeCloseTo(.5*f,1);expect(shape.box.y).toBeCloseTo(.5*f,1);
 expect(shape.box.bottom).toBeCloseTo(900-.5*f,1);expect(shape.radius).toBeGreaterThanOrEqual(f);
 const handle=page.getByRole('separator',{name:'사이드바 너비',exact:true});
 const initial=Number(await handle.getAttribute('aria-valuenow'));
 await drag(page,handle,60);
 await expect.poll(()=>slot.evaluate(e=>e.getBoundingClientRect().width)).toBeCloseTo(initial+60,0);
 const saved=await slot.evaluate(e=>e.getBoundingClientRect().width);
 await page.reload();await expect.poll(()=>slot.evaluate(e=>e.getBoundingClientRect().width)).toBeCloseTo(saved,0);
 await handle.press('Home');await expect(handle).toHaveAttribute('aria-valuenow',String(Math.round(14*f)));
 await handle.press('End');await expect(handle).toHaveAttribute('aria-valuenow',await handle.getAttribute('aria-valuemax') as string);
 await handle.dblclick();await expect(handle).toHaveAttribute('aria-valuenow',String(Math.round(18*f)));
 await page.getByRole('button',{name:'사이드바 닫기',exact:true}).click();
 await expect.poll(()=>slot.evaluate(e=>e.clientWidth)).toBe(0);
 await expect(page.getByRole('navigation',{name:'주요 메뉴'})).toHaveCount(0);
 expect((await page.locator('.workspace-shell').boundingBox())!.x).toBe(0);
 await page.getByRole('button',{name:'사이드바 열기',exact:true}).click();
 await expect(handle).toBeVisible();await bounded(page);
});

test('resizing the canvas inspector preserves camera center and leaves the board uncovered',async({page})=>{
 await page.setViewportSize({width:1600,height:900});await page.goto(board);
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();const original=await camera(page);
 await page.locator('[data-session-id="layout-child"]').click();
 const handle=page.getByRole('separator',{name:'세션 상세 너비',exact:true});
 const initial=Number(await handle.getAttribute('aria-valuenow'));await drag(page,handle,-72);
 await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBe(initial+72);
 await expect.poll(async()=>(await camera(page)).x).toBeCloseTo(original.x,1);
 await expect.poll(async()=>(await camera(page)).y).toBeCloseTo(original.y,1);
 const b=(await page.locator('.board').boundingBox())!,p=(await page.locator('.run-details').boundingBox())!;
 expect(b.x+b.width).toBeLessThanOrEqual(p.x);
 await handle.press('ArrowRight');await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBeLessThan(initial+72);
 await page.getByRole('button',{name:'상세 닫기',exact:true}).click();
 await expect.poll(async()=>(await camera(page)).zoom).toBeCloseTo(original.zoom,5);
 await bounded(page);
});

for(const width of [1600,390])test(`context divider resizes the ${width===390?'bottom':'right'} panel and survives reload`,async({page})=>{
 await page.setViewportSize({width,height:width===390?1400:900});await page.goto(chat);
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 const stacked=width===390,handle=page.getByRole('separator',{name:stacked?'업무 맥락 높이':'업무 맥락 너비',exact:true});
 const old=Number(await handle.getAttribute('aria-valuenow'));
 await drag(page,handle,stacked?0:-50,stacked?-50:0);
 await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBe(old+50);
 const name=stacked?'contextHeight':'context';
 const saved=await page.evaluate(key=>JSON.parse(localStorage.getItem('bibi:panels')!)[key],name);
 await page.reload();await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 await expect(handle).toHaveAttribute('aria-valuenow',String(Math.round(saved*await font(page))));
 await handle.press('End');const maximum=Number(await handle.getAttribute('aria-valuenow'));
 await page.reload();await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 await expect(handle).toHaveAttribute('aria-valuenow',String(maximum));
 await handle.press(stacked?'ArrowDown':'ArrowRight');
 await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBeLessThan(maximum);
 const c=(await page.locator('.conversation-panel').boundingBox())!,p=(await page.locator('.context-panel').boundingBox())!;
 expect(stacked?c.y+c.height<=p.y:c.x+c.width<=p.x).toBe(true);
 expect((await page.locator('.messages').boundingBox())!.height).toBeGreaterThan(100);
 await page.getByRole('button',{name:'맥락 닫기',exact:true}).click();
 await expect(page.getByRole('complementary',{name:'업무 맥락 내용'})).toHaveCount(0);
 await bounded(page);
});

test('Windows text, buttons, fields and icons use consistent dimensions',async({page})=>{
 await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{value:'Win32'}));
 await page.setViewportSize({width:1440,height:900});await page.goto(chat);
 await expect.poll(()=>font(page)).toBe(16);
 expect(await page.locator('.message-text').first().evaluate(e=>parseFloat(getComputedStyle(e).fontSize))).toBeGreaterThanOrEqual(16);
 await page.getByRole('button',{name:'전송 설정',exact:true}).click();
 const heights=await page.locator('.composer-options>input,.composer-options>button,.composer-settings select,.composer-settings>button,.session-action-buttons>button').evaluateAll(elements=>elements.map(e=>e.getBoundingClientRect().height));
 expect(heights.length).toBeGreaterThan(7);for(const height of heights)expect(height).toBeCloseTo(40,1);
 const icons=await page.locator('.icon-button:visible').evaluateAll(buttons=>buttons.map(button=>{
  const b=button.getBoundingClientRect(),svg=button.querySelector('svg')!.getBoundingClientRect();
  return {button:b.width,height:b.height,icon:svg.width,iconHeight:svg.height,x:svg.x+svg.width/2-b.x-b.width/2,y:svg.y+svg.height/2-b.y-b.height/2};
 }));
 for(const icon of icons){expect(icon.button).toBe(40);expect(icon.height).toBe(40);expect(icon.icon).toBe(20);expect(icon.iconHeight).toBe(20);expect(Math.abs(icon.x)).toBeLessThan(.6);expect(Math.abs(icon.y)).toBeLessThan(.6);}
 await sidebarAction(page,'설정');const dialog=page.getByRole('dialog',{name:'설정',exact:true});
 await dialog.getByRole('button',{name:'제공자 추가',exact:true}).click();
 const dimensions=await dialog.locator('.provider-editor input:not([type=checkbox]),.provider-editor select').evaluateAll(elements=>elements.map(e=>e.getBoundingClientRect().height));
 for(const height of dimensions)expect(height).toBeCloseTo(40,1);
 await bounded(page);
});

async function startSamples(page:Page,selector:string,axis:'width'|'height'){
 await page.evaluate(({selector,axis})=>{
  const samples:number[]=[];(window as unknown as {sizeSamples:number[]}).sizeSamples=samples;
  let remaining=400;
  function sample(){const e=document.querySelector(selector);samples.push(e?.getBoundingClientRect()[axis]??0);if(--remaining)requestAnimationFrame(sample);}
  requestAnimationFrame(sample);
 },{selector,axis});
}
async function intermediateFrames(page:Page,min:number,max:number){
 const samples=await page.evaluate(()=>(window as unknown as {sizeSamples:number[]}).sizeSamples);
 expect(new Set(samples.filter(v=>v>min+2&&v<max-2).map(Math.round)).size).toBeGreaterThan(3);
}
for(const width of [1600,390])test(`context panel has opening and closing layout frames at ${width}px`,async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});await page.setViewportSize({width,height:900});await page.goto(chat);
 await expect(page.locator('.conversation-panel')).toBeVisible();
 expect(await page.locator('.conversation-layout').evaluate(e=>getComputedStyle(e).transitionDuration.split(',').map(parseFloat))).toEqual([.26,.26]);
 await page.addStyleTag({content:':root { --panel-duration: 800ms; }'});
 const axis=width===390?'height':'width',pane=page.locator('.conversation-inspector');
 await startSamples(page,'.conversation-inspector',axis);
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 const expected=await page.locator('.app-shell').evaluate((e,key)=>parseFloat(e.style.getPropertyValue(key))*parseFloat(getComputedStyle(e).fontSize),width===390?'--context-height':'--context-width');
 await expect.poll(()=>pane.evaluate((e,a:'width'|'height')=>e.getBoundingClientRect()[a],axis)).toBeCloseTo(expected,0);
 await intermediateFrames(page,0,expected);
 await startSamples(page,'.conversation-inspector',axis);
 await page.getByRole('button',{name:'맥락 닫기',exact:true}).click();
 await expect.poll(()=>pane.evaluate((e,a:'width'|'height')=>e.getBoundingClientRect()[a],axis)).toBe(0);
 await intermediateFrames(page,0,expected);await bounded(page);
});

test('composer disclosure and native details animate; reduced motion is immediate',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});await page.setViewportSize({width:1600,height:900});await page.goto(chat);
 await page.addStyleTag({content:':root { --panel-duration: 800ms; }'});
 await startSamples(page,'.composer-settings','height');
 await page.getByRole('button',{name:'전송 설정',exact:true}).click();
 const settings=page.locator('.composer-settings');
 const controlHeight=await page.locator('.composer-options>input').evaluate(e=>e.getBoundingClientRect().height);
 await expect.poll(()=>settings.evaluate(e=>e.getBoundingClientRect().height)).toBeCloseTo(controlHeight,1);
 await expect.poll(()=>settings.evaluate(e=>e.getAnimations().length)).toBe(0);
 await intermediateFrames(page,0,(await settings.boundingBox())!.height);
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 const summary=page.locator('.context-panel summary').filter({hasText:'실행 사용량'}),details=summary.locator('..');
 await summary.scrollIntoViewIfNeeded();const closed=(await details.boundingBox())!.height;
 await startSamples(page,'.context-panel details:last-child','height');await summary.click();
 await expect.poll(()=>details.evaluate(e=>e.getAnimations().length)).toBe(0);
 const expanded=(await details.boundingBox())!.height;expect(expanded).toBeGreaterThan(closed+30);await intermediateFrames(page,closed,expanded);
 await page.emulateMedia({reducedMotion:'reduce'});await summary.click();await expect(details).not.toHaveAttribute('open');
 await page.getByRole('button',{name:'맥락 닫기',exact:true}).click();
 expect(await page.locator('.conversation-layout').evaluate(e=>getComputedStyle(e).transitionDuration)).toBe('0s');
 await expect(page.locator('.context-panel')).toBeHidden();
 await page.getByRole('button',{name:'전송 설정',exact:true}).click();await expect(settings).toHaveCount(0);
});

test('animated mobile drawer keeps its scrollbar inside and restores focus on close',async({page})=>{
 await page.emulateMedia({reducedMotion:'no-preference'});await page.setViewportSize({width:390,height:640});await page.goto(chat);
 const opener=page.getByRole('button',{name:'사이드바 열기',exact:true});await opener.click();
 const drawer=page.getByRole('dialog',{name:'사이드바',exact:true}),inset=.5*await font(page);
 await expect.poll(()=>drawer.evaluate(e=>e.getBoundingClientRect().x)).toBeCloseTo(inset,1);
 const thumb=drawer.getByRole('scrollbar',{name:'탐색 세로 스크롤',exact:true});
 await expect(thumb).toBeVisible();
 await expect.poll(async()=>{
  const d=(await drawer.boundingBox())!,t=(await thumb.boundingBox())!;
  return t.x>=d.x&&t.x+t.width<=d.x+d.width&&t.y>=d.y&&t.y+t.height<=d.y+d.height;
 }).toBe(true);
 await page.keyboard.press('Escape');await expect(drawer).toHaveCount(0);await expect(opener).toBeFocused();
 await bounded(page);
});

for(const platform of ['MacIntel','Win32'])for(const width of [1440,390])test(`action buttons keep their shape and alignment on ${platform} at ${width}px`,async({page})=>{
 await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
 await page.setViewportSize({width,height:900});await page.goto(chat);const f=await font(page);
 const rename=page.getByRole('button',{name:'세션 이름 변경',exact:true}),before=(await rename.boundingBox())!;
 await rename.click();const editor=page.locator('.conversation-heading .inline-confirm');
 await expect(editor).toBeVisible();
 await expect.poll(async()=>(await rename.boundingBox())!.x).toBeCloseTo(before.x,1);
 await expect.poll(async()=>(await rename.boundingBox())!.y).toBeCloseTo(before.y,1);
 const input=(await editor.getByRole('textbox',{name:'새 세션 이름'}).boundingBox())!,cancel=(await editor.getByRole('button',{name:'취소',exact:true}).boundingBox())!,save=(await editor.getByRole('button',{name:'저장',exact:true}).boundingBox())!;
 expect(cancel.x).toBeLessThan(save.x);
 expect(cancel.y+cancel.height/2).toBeCloseTo(input.y+input.height/2,1);
 expect(save.y+save.height/2).toBeCloseTo(input.y+input.height/2,1);
 const shapes=await page.locator('.session-action-buttons>button,.inline-confirm button,.composer-options>.send').evaluateAll(elements=>elements.map(e=>({height:e.getBoundingClientRect().height,radius:parseFloat(getComputedStyle(e).borderTopLeftRadius)})));
 expect(shapes.length).toBe(5);
 for(const shape of shapes){expect(shape.radius).toBeGreaterThanOrEqual(shape.height/2);expect(shape.height).toBeCloseTo(save.height,1);}
 await editor.getByRole('button',{name:'취소',exact:true}).click();
 await sidebarAction(page,'설정');const dialog=page.getByRole('dialog',{name:'설정',exact:true});
 const reset=(await dialog.getByRole('button',{name:'100%로 복원',exact:true}).boundingBox())!,label=(await dialog.locator('label[for="ui-scale"]').boundingBox())!;
 expect(reset.y+reset.height/2).toBeCloseTo(label.y+label.height/2,1);
 await dialog.getByRole('button',{name:'제공자 추가',exact:true}).click();
 const form=dialog.locator('.provider-editor'),actions=form.locator(':scope>.form-actions');
 await actions.scrollIntoViewIfNeeded();
 const footer=(await actions.boundingBox())!,secondary=(await actions.getByRole('button',{name:'취소',exact:true}).boundingBox())!,primary=(await actions.getByRole('button',{name:'저장',exact:true}).boundingBox())!;
 expect(primary.x+primary.width).toBeCloseTo(footer.x+footer.width,1);
 expect(primary.x-secondary.x-secondary.width).toBeCloseTo(.5*f,1);
 expect(primary.y+primary.height/2).toBeCloseTo(secondary.y+secondary.height/2,1);
 for(const button of await actions.getByRole('button').all())expect(await button.evaluate(e=>parseFloat(getComputedStyle(e).borderTopLeftRadius))).toBeGreaterThanOrEqual(primary.height/2);
 await bounded(page);
});
