import {test as base,expect,type Page} from '@playwright/test';
import {sidebarAction,showSidebar} from './navigation';

const test=base.extend<{uiErrors:string[]}>({
 uiErrors:[async({page,baseURL},use)=>{
  const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',route=>{
   if(new URL(route.request().url()).origin===new URL(baseURL!).origin&&route.request().method()==='GET')return route.continue();
   errors.push('unexpected request '+route.request().url());return route.abort();
  });
  await use(errors);expect(errors).toEqual([]);
 },{auto:true}]
});
test.beforeEach(async({page},info)=>{
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
});
const chat='/?view=conversation&project=layout-project&run=layout-parent';
const board='/?view=canvas&project=layout-project';
const back=(page:Page)=>page.getByRole('button',{name:'뒤로가기',exact:true});
const forward=(page:Page)=>page.getByRole('button',{name:'앞으로가기',exact:true});
async function view(page:Page,name:string){await expect(page.locator('main.'+name)).toBeVisible();}

test('first entry cannot leave BiBi and repeated navigation does not add a step',async({page})=>{
 await page.goto('/__ready');await page.goto(chat);await view(page,'conversation');
 await expect(back(page)).toBeDisabled();await expect(forward(page)).toBeDisabled();
 const original=page.url();await back(page).evaluate(e=>(e as HTMLButtonElement).click());expect(page.url()).toBe(original);
 await sidebarAction(page,'작업 대화');await expect(back(page)).toBeDisabled();
 await sidebarAction(page,'사용량·연결');await view(page,'usage');await expect(back(page)).toBeEnabled();
 await sidebarAction(page,'사용량·연결');
 await back(page).evaluate(e=>{(e as HTMLButtonElement).click();(e as HTMLButtonElement).click();});
 await view(page,'conversation');
 await expect(back(page)).toBeDisabled();await expect(forward(page)).toBeEnabled();
 await forward(page).focus();await page.keyboard.press('Enter');await view(page,'usage');
 await expect(forward(page)).toBeDisabled();
});

test('buttons and browser history share menu, session, group and project navigation',async({page})=>{
 await page.route('**/api/snapshot',async route=>{
  const response=await route.fetch(),data=await response.json();
  data.projects.push({...data.projects[0],id:'second-project',name:'두 번째 프로젝트'});
  await route.fulfill({response,json:data});
 });
 await page.goto(board);await view(page,'canvas');await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await page.locator('[data-session-id="layout-parent"]').dblclick();await view(page,'conversation');
 await back(page).click();await view(page,'canvas');await expect(page.getByRole('button',{name:'대화 열기',exact:true})).toBeVisible();
 await back(page).click();await expect(back(page)).toBeDisabled();
 await forward(page).click();await expect(page.getByRole('button',{name:'대화 열기',exact:true})).toBeVisible();
 await page.goForward();await view(page,'conversation');await expect(forward(page)).toBeDisabled();
 await page.goBack();await view(page,'canvas');
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await page.locator('.work-group').first().dblclick({position:{x:10,y:10}});
 await expect(page).toHaveURL(/group=/);await back(page).click();await expect(page.locator('.work-group')).toHaveCount(2);
 await forward(page).click();await expect(page.locator('.work-group')).toHaveCount(1);
 await showSidebar(page);await page.getByRole('combobox',{name:'프로젝트',exact:true}).selectOption('second-project');
 await expect(page).toHaveURL(/project=second-project/);await back(page).click();
 await expect(page).toHaveURL(/project=layout-project/);await expect(page.locator('.work-group')).toHaveCount(1);
 await forward(page).click();await expect(page).toHaveURL(/project=second-project/);
});

for(const reducedMotion of ['reduce','no-preference'] as const)for(const session of ['layout-parent','layout-expert'])test(`node double-click opens through inspector reflow (${reducedMotion}, ${session})`,async({page})=>{
 await page.emulateMedia({reducedMotion});await page.goto(board);await view(page,'canvas');
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 // Give the first-click layout time to move the node before the second click.
 // Both clicks still use the original screen position, like a real double-click.
 await page.locator(`[data-session-id="${session}"]`).dblclick({delay:80});
 await view(page,'conversation');await expect(page).toHaveURL(new RegExp('run='+session));
 await back(page).click();await view(page,'canvas');await expect(page).toHaveURL(new RegExp('run='+session));
 await back(page).click();await expect(back(page)).toBeDisabled();
});

test('separate clicks and background double-clicks do not open a stale node',async({page})=>{
 await page.goto(board);await view(page,'canvas');await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await page.locator('[data-session-id="layout-parent"]').click();await expect(page).toHaveURL(/run=layout-parent/);
 const expert=page.locator('[data-session-id="layout-expert"]');await expert.click();await expect(page).toHaveURL(/run=layout-expert/);await view(page,'canvas');
 const box=(await page.locator('.board').boundingBox())!;
 await page.mouse.dblclick(box.x+12,box.y+box.height-80);await view(page,'canvas');await expect(page).toHaveURL(/run=layout-expert/);
 await expert.press('Enter');await view(page,'conversation');await expect(page).toHaveURL(/run=layout-expert/);
});

test('middle-entry reload restores both directions and keeps the chat draft',async({page})=>{
 await page.goto(chat);await page.getByRole('textbox',{name:'메시지',exact:true}).fill('이전 대화에 남길 초안');
 await sidebarAction(page,'사용량·연결');await sidebarAction(page,'세션 캔버스');
 await back(page).click();await view(page,'usage');await page.reload();await view(page,'usage');
 await expect(back(page)).toBeEnabled();await expect(forward(page)).toBeEnabled();
 await back(page).click();await view(page,'conversation');
 await expect(page.getByRole('textbox',{name:'메시지',exact:true})).toHaveValue('이전 대화에 남길 초안');
 await expect(back(page)).toBeDisabled();await forward(page).click();await view(page,'usage');
 await forward(page).click();await view(page,'canvas');await expect(forward(page)).toBeDisabled();
});

test('new navigation after going back discards the old forward branch',async({page})=>{
 await page.goto(chat);await sidebarAction(page,'사용량·연결');await sidebarAction(page,'세션 캔버스');
 await back(page).click();await view(page,'usage');await sidebarAction(page,'작업 대화');
 await expect(forward(page)).toBeDisabled();await page.reload();await view(page,'conversation');
 await expect(forward(page)).toBeDisabled();await back(page).click();await view(page,'usage');
 await forward(page).click();await view(page,'conversation');await expect(forward(page)).toBeDisabled();
});

test('modal close, browser traversal and reload retain the same navigation range',async({page})=>{
 await page.goto(chat);await sidebarAction(page,'설정');
 const dialog=page.getByRole('dialog',{name:'설정',exact:true});await expect(dialog).toBeVisible();
 await page.reload();await expect(dialog).toBeVisible();await dialog.getByRole('button',{name:'닫기',exact:true}).click();
 await expect(dialog).toHaveCount(0);await expect(back(page)).toBeDisabled();await expect(forward(page)).toBeEnabled();
 await forward(page).click();await expect(dialog).toBeVisible();await page.goBack();await expect(dialog).toHaveCount(0);
 await page.goForward();await expect(dialog).toBeVisible();await page.keyboard.press('Escape');
 await expect(dialog).toHaveCount(0);await expect(back(page)).toBeDisabled();await expect(forward(page)).toBeEnabled();
});

test('a direct modal link closes in place without leaving the app',async({page})=>{
 await page.goto('/__ready');await page.goto(chat+'&modal=settings');
 const dialog=page.getByRole('dialog',{name:'설정',exact:true});await expect(dialog).toBeVisible();
 await dialog.getByRole('button',{name:'닫기',exact:true}).click();await expect(dialog).toHaveCount(0);
 await view(page,'conversation');await expect(back(page)).toBeDisabled();await expect(forward(page)).toBeDisabled();
});

test('blocked session storage still allows navigation during the open page',async({page})=>{
 await page.addInitScript(()=>Object.defineProperty(window,'sessionStorage',{get(){throw new DOMException('Blocked','SecurityError');}}));
 await page.goto(chat);await sidebarAction(page,'사용량·연결');await back(page).click();await view(page,'conversation');
 await expect(back(page)).toBeDisabled();await expect(forward(page)).toBeEnabled();
 await forward(page).click();await view(page,'usage');await expect(forward(page)).toBeDisabled();
});

for(const width of [1440,390,320])for(const scale of [1,2])test(`navigation buttons remain aligned and accessible at ${width}px / ${scale*100}%`,async({page})=>{
 await page.setViewportSize({width,height:900});
 await page.addInitScript(scale=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:scale})),scale);
 await page.goto(board);await view(page,'canvas');
 const f=await page.evaluate(()=>navigator.platform.startsWith('Win')?16:14);
 await expect.poll(()=>page.evaluate(()=>parseFloat(getComputedStyle(document.documentElement).fontSize))).toBe(f*scale);
 for(const button of [back(page),forward(page)]){
  await expect(button).toBeVisible();await expect(button).toBeInViewport();
  const geometry=await button.evaluate(e=>{
   const b=e.getBoundingClientRect(),icon=e.querySelector('svg')!.getBoundingClientRect(),bar=e.closest('header')!.getBoundingClientRect();
   return {x:icon.x+icon.width/2-b.x-b.width/2,y:icon.y+icon.height/2-b.y-b.height/2,width:b.width,height:b.height,left:b.left-bar.left,right:bar.right-b.right};
  });
  expect(geometry.width).toBeCloseTo(geometry.height,1);expect(Math.abs(geometry.x)).toBeLessThan(.6);expect(Math.abs(geometry.y)).toBeLessThan(.6);
  expect(geometry.left).toBeGreaterThanOrEqual(0);expect(geometry.right).toBeGreaterThanOrEqual(0);
 }
 const buttons=await page.locator('.content-toolbar button:visible').evaluateAll(es=>es.map(e=>e.getBoundingClientRect().toJSON()));
 for(let i=0;i<buttons.length;i++)for(let j=i+1;j<buttons.length;j++){
  const a=buttons[i],b=buttons[j];expect(Math.min(a.right,b.right)-Math.max(a.left,b.left)<=1||Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)<=1,'toolbar buttons must not overlap').toBe(true);
 }
 expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
 expect(await page.evaluate(()=>document.documentElement.scrollHeight-innerHeight)).toBeLessThanOrEqual(1);
 await sidebarAction(page,'사용량·연결');await back(page).click();await view(page,'canvas');
});
