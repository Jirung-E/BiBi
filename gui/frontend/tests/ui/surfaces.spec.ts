import {test as base,expect,type Page,type Locator} from '@playwright/test';

const test=base.extend<{errors:string[]}>({errors:[async({page,baseURL},use)=>{
 const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',route=>{
  const request=route.request();
  if(new URL(request.url()).origin===new URL(baseURL!).origin&&request.method()==='GET')return route.continue();
  errors.push('unexpected request '+request.url());return route.abort();
 });
 await use(errors);expect(errors).toEqual([]);
},{auto:true}]});
test.beforeEach(async({page},info)=>{
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
});
async function withImportProvider(page:Page){
 await page.route('**/api/snapshot',async route=>{
  const response=await route.fetch(),data=await response.json();
  data.providers=[{...data.providers[0],adapter:'codex',name:'Codex'}];
  await route.fulfill({response,json:data});
 });
}
async function open(page:Page,view:string,width:number,height=844){
 await page.setViewportSize({width,height});
 await page.goto('/?view='+view+'&project=layout-project'+(view==='usage'?'':'&run=layout-parent'));
 await expect(page.locator('.main-content')).toBeVisible();
 if(view==='conversation')await expect(page.getByRole('textbox',{name:'메시지',exact:true})).toBeVisible();
}
async function balancedToolbar(page:Page,surface:Locator){
 const geometry=await page.locator('.content-toolbar').evaluate(e=>{
  const controls=[...e.querySelectorAll<HTMLElement>('.sidebar-toggle,.toolbar-actions>button,.session-import>summary')]
   .map(control=>control.getBoundingClientRect()).filter(rect=>rect.width>0&&rect.height>0);
  return {count:controls.length,top:Math.min(...controls.map(rect=>rect.top))-e.parentElement!.getBoundingClientRect().top,bottom:Math.max(...controls.map(rect=>rect.bottom))};
 });
 expect(geometry.count).toBeGreaterThan(0);
 expect(geometry.top).toBeGreaterThan(0);
 const below=(await surface.boundingBox())!.y-geometry.bottom;
 expect(below,'toolbar actions retain breathing room above the content').toBeGreaterThan(0);
 expect(Math.abs(below-geometry.top),'visible controls have equal window-top and content gaps').toBeLessThanOrEqual(1);
}
async function balancedSurface(page:Page){
 await balancedToolbar(page,page.locator('.main-content').locator(':scope > :first-child'));
 const geometry=await page.locator('.main-content').evaluate(e=>{
  const area=e.getBoundingClientRect(),body=e.firstElementChild!.getBoundingClientRect();
  return {left:body.left-area.left,right:area.right-body.right,bottom:area.bottom-body.bottom,font:parseFloat(getComputedStyle(document.documentElement).fontSize),width:document.documentElement.scrollWidth,height:document.documentElement.scrollHeight,windowWidth:innerWidth,windowHeight:innerHeight};
 });
 // The toolbar supplies the top spacing; the remaining workspace edges share one inset.
 expect(geometry.left).toBeGreaterThanOrEqual(geometry.font*.4);
 expect(geometry.left).toBeLessThanOrEqual(geometry.font*.8);
 expect(Math.abs(geometry.left-geometry.right)).toBeLessThanOrEqual(1);
 if(!(await page.locator('.main-content').getAttribute('class'))?.includes('usage'))expect(Math.abs(geometry.left-geometry.bottom)).toBeLessThanOrEqual(1);
 expect(geometry.width).toBeLessThanOrEqual(geometry.windowWidth+1);
 expect(geometry.height).toBeLessThanOrEqual(geometry.windowHeight+1);
}
for(const width of [390,1280])for(const theme of ['light','dark'] as const)test(`workspace surfaces keep balanced insets at ${width}px ${theme}`,async({page})=>{
 await withImportProvider(page);
 await page.emulateMedia({colorScheme:theme});
 for(const view of ['conversation','canvas','usage']){
  await open(page,view,width);await balancedSurface(page);
 }
 const host=page.locator('.host-panel').first();
 const title=(await host.getByRole('heading',{name:'호스트',exact:true}).boundingBox())!;
 const action=(await host.getByRole('button',{name:'호스트 연결',exact:true}).boundingBox())!;
 expect(Math.abs(title.y+title.height/2-action.y-action.height/2)).toBeLessThanOrEqual(1);
 const gaps=await page.locator('.main-content.usage').evaluate(e=>{
  const first=e.querySelector('.quota-grid')!.getBoundingClientRect(),cards=[...e.querySelectorAll('.host-panel')].map(c=>c.getBoundingClientRect());
  return [cards[0].top-first.bottom,cards[1].top-cards[0].bottom];
 });
 expect(gaps[0]).toBeGreaterThan(0);expect(gaps[0]).toBeCloseTo(gaps[1],1);
});
for(const width of [390,1280])for(const scale of [.5,2])test(`content gutters survive ${width}px UI ${scale*100}%`,async({page})=>{
 await withImportProvider(page);
 await page.addInitScript(value=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:value,halfLife:30,floor:.15})),scale);
 for(const view of ['canvas','usage','conversation']){await open(page,view,width);await balancedSurface(page);}
 const send=page.getByRole('button',{name:'전송',exact:true});await send.scrollIntoViewIfNeeded();await expect(send).toBeInViewport();
});
async function scrollKeepsHeader(page:Page,body:Locator,header:Locator,close:Locator){
 await expect(body).toBeVisible();
 const initial=(await close.boundingBox())!,headerBefore=(await header.boundingBox())!,bodyBefore=(await body.boundingBox())!;
 const font=await page.evaluate(()=>parseFloat(getComputedStyle(document.documentElement).fontSize));
 expect(bodyBefore.y-headerBefore.y-headerBefore.height).toBeGreaterThanOrEqual(font*.4);
 await body.evaluate(e=>e.scrollTop=e.scrollHeight);await expect.poll(()=>body.evaluate(e=>e.scrollTop)).toBeGreaterThan(0);
 const after=(await close.boundingBox())!,headerAfter=(await header.boundingBox())!,bodyAfter=(await body.boundingBox())!;
 expect(after.y).toBeCloseTo(initial.y,1);expect(headerAfter.y).toBeCloseTo(headerBefore.y,1);expect(bodyAfter.y).toBeCloseTo(bodyBefore.y,1);
 await expect(close).toBeInViewport();
}
for(const width of [390,1600])for(const theme of ['light','dark'] as const)test(`panel controls and scroll gutters stay separate at ${width}px ${theme}`,async({page})=>{
 await page.emulateMedia({colorScheme:theme});
 await page.route('**/api/snapshot',async route=>{
  const data=await(await route.fetch()).json();
  for(const work of data.works)work.constraints.push(...Array.from({length:30},(_,i)=>'스크롤 확인 항목 '+i));
  await route.fulfill({json:data});
 });
 await open(page,'canvas',width,640);
 const details=page.getByRole('complementary',{name:'세션 상세',exact:true});
 await details.locator('summary').filter({hasText:'지원 기능'}).click();
 await scrollKeepsHeader(page,details.getByLabel('세션 상세 내용',{exact:true}),details.locator('.run-details-toolbar'),page.getByRole('button',{name:'상세 닫기',exact:true}));
 await open(page,'conversation',width,640);await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 const context=page.getByRole('complementary',{name:'업무 맥락 내용',exact:true});
 await scrollKeepsHeader(page,context.getByLabel('맥락 기록',{exact:true}),context.locator('.panel-header'),page.getByRole('button',{name:'맥락 닫기',exact:true}));
 const messages=page.getByLabel('대화 기록',{exact:true}),heading=page.getByLabel('세션 정보',{exact:true});
 await scrollKeepsHeader(page,messages,heading,page.getByRole('button',{name:'세션 이름 변경',exact:true}));
 const messageBox=(await messages.boundingBox())!,composerBox=(await page.getByRole('form',{name:'메시지 작성',exact:true}).boundingBox())!;
 expect(composerBox.y-messageBox.y-messageBox.height).toBeGreaterThan(0);
});
test('an error notice has the same inset as the workspace and leaves controls reachable',async({page})=>{
 await page.route('**/api/runs/*',route=>route.fulfill({status:500,json:{error:'화면 경계 검증용 오류'}}));
 await open(page,'conversation',390);
 const notice=page.getByRole('alert');await expect(notice).toContainText('화면 경계 검증용 오류');
 await balancedToolbar(page,notice);
 const box=(await notice.boundingBox())!,content=(await page.locator('.conversation-panel').boundingBox())!;
 expect(content.y-box.y-box.height).toBeGreaterThan(0);
 expect(box.x).toBeCloseTo(content.x,1);expect(box.width).toBeCloseTo(content.width,1);
 await page.getByRole('button',{name:'오류 닫기',exact:true}).click();await expect(notice).toHaveCount(0);await balancedSurface(page);
});

test('a narrow enlarged inspector keeps both its fixed actions and body accessible',async({page})=>{
 await page.addInitScript(()=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:2,halfLife:30,floor:.15})));
 await open(page,'canvas',320,640);
 const details=page.getByRole('complementary',{name:'세션 상세',exact:true});
 const close=page.getByRole('button',{name:'상세 닫기',exact:true}),openChat=page.getByRole('button',{name:'대화 열기',exact:true});
 await expect(close).toBeInViewport();await expect(openChat).toBeInViewport();
 const box=(await details.boundingBox())!,header=(await details.locator('.panel-header').boundingBox())!,body=(await details.getByLabel('세션 상세 내용',{exact:true}).boundingBox())!;
 expect(body.height).toBeGreaterThan(32);
 expect(body.y).toBeGreaterThan(header.y+header.height);
 expect(body.y+body.height).toBeLessThanOrEqual(box.y+box.height);
 const original=(await close.boundingBox())!;
 await details.getByLabel('세션 상세 내용',{exact:true}).evaluate(e=>e.scrollTop=e.scrollHeight);
 expect((await close.boundingBox())!.y).toBeCloseTo(original.y,1);
});
