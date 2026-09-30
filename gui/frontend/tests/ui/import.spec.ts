import {test as base,expect,type Page} from '@playwright/test';
import {showSidebar} from './navigation';
import {expectOverlayScrolling} from './scrolling';

type Reply={imported:number;errors:{session:string;error:string}[];scope:{workspace:string;host:string;max_sessions:number;active_control:boolean}};
type Wire={calls:Record<string,string>[];reads:number;reply:Reply;error:string;wait:Promise<void>|null;failRefresh:boolean;completed:boolean};
const test=base.extend<{wire:Wire}>({wire:[async({page,baseURL},use)=>{
 const errors:string[]=[],wire:Wire={calls:[],reads:0,reply:{imported:1,errors:[],scope:{workspace:'/fixture/saved-sessions',host:'local',max_sessions:2000,active_control:false}},error:'',wait:null,failRefresh:false,completed:false};
 page.on('pageerror',error=>errors.push(error.message));
 await page.route('**/*',async route=>{
  const request=route.request(),url=new URL(request.url());
  if(url.origin===new URL(baseURL!).origin){
   if(request.method()==='GET'){
    if(url.pathname==='/api/snapshot'){
     wire.reads++;
     if(wire.completed&&wire.failRefresh)return route.fulfill({status:503,json:{error:'목록 서버가 잠시 응답하지 않습니다.'}});
     const response=await route.fetch(),data=await response.json();
     data.projects[0].workspace='/fixture/saved-sessions';
     data.projects.push({...data.projects[0],id:'other-project',name:'다른 프로젝트',workspace:'/fixture/other-project'});
     data.providers=[{...data.providers[0],adapter:'codex',name:'Codex'}];
     if(wire.completed&&wire.reply.imported){
      const run={...data.runs[0],id:'imported-session',session_id:'imported-session',project_key:wire.calls.at(-1)?.project_key,work_id:'imported-work',title:'가져온 외부 세션',origin:'external',provider:'codex'};
      data.runs.push(run);data.works.push({...data.works[0],id:run.work_id,project_key:run.project_key,title:run.title});
     }
     return route.fulfill({response,json:data});
    }
    return route.continue();
   }
   if(request.method()==='POST'&&url.pathname==='/api/command'){
    const body=request.postDataJSON();
    if(body.type==='discover'){
     wire.calls.push(body);await wire.wait;
     if(wire.error)return route.fulfill({status:502,json:{error:wire.error}});
     wire.completed=true;
     return route.fulfill({json:wire.reply});
    }
   }
  }
  errors.push(`unexpected request: ${request.method()} ${request.url()}`);return route.abort();
 });
 await use(wire);expect(errors).toEqual([]);
},{auto:true}]});

test.beforeEach(async({page},info)=>{
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
});
async function open(page:Page,width=390,scale=1){
 await page.setViewportSize({width,height:844});
 await page.addInitScript(value=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:value})),scale);
 await page.goto('/?view=canvas&project=layout-project&group=layout-work');
 await page.getByLabel('외부 세션 가져오기',{exact:true}).click();
 return page.getByLabel('외부 세션 제공자',{exact:true});
}

for(const width of [390,1280])test(`import reports pending once and refreshes without stream events at ${width}px`,async({page,wire})=>{
 let release!:()=>void;wire.wait=new Promise<void>(resolve=>release=resolve);
 const menu=await open(page,width),provider=menu.getByRole('button',{name:'Codex',exact:true});
 try{
  await provider.click();
  await expect(menu.getByRole('status')).toHaveText('Codex 세션 조회 중…');
  await expect(provider).toBeDisabled();expect(wire.calls).toEqual([{type:'discover',project_key:'layout-project',provider:'codex',provider_id:'layout-provider'}]);
  await page.getByLabel('외부 세션 가져오기',{exact:true}).click();
  await page.getByLabel('외부 세션 가져오기',{exact:true}).click();
  await expect(provider).toBeDisabled();expect(wire.calls).toHaveLength(1);
 }finally{release();}
 await expect(menu.getByRole('status')).toHaveText('세션 1개 가져옴');
 await expect(provider).toBeEnabled();expect(wire.reads).toBeGreaterThan(1);
 await menu.getByRole('button',{name:'캔버스에서 보기',exact:true}).click();
 await expect(menu).toBeHidden();await expect(page).not.toHaveURL(/group=/);
 await expect(page.locator('[data-session-id="imported-session"]')).toBeInViewport();
 await expect(page.locator('[data-session-id="imported-session"] strong')).toHaveText('가져온 외부 세션');
 expect(wire.calls).toHaveLength(1);
});

test('empty discovery explains its folder scope and can be retried',async({page,wire})=>{
 wire.reply.imported=0;
 const menu=await open(page);
 expect((await menu.locator('.import-scope>summary').boundingBox())!.height).toBeGreaterThanOrEqual(44);
 await menu.getByText('조회 폴더',{exact:true}).click();
 await expect(menu.locator('.import-scope code')).toBeVisible();
 await expect(menu).toContainText('BiBi 실행 호스트 · 현재 프로젝트 폴더');
 await expect(menu.locator('.import-scope code')).toHaveText('/fixture/saved-sessions');
 await menu.getByRole('button',{name:'Codex',exact:true}).click();
 await expect(menu.getByRole('status')).toHaveText('가져올 외부 세션이 없습니다.');
 await expect(menu.getByRole('button',{name:'캔버스에서 보기'})).toHaveCount(0);
 wire.reply.imported=1;
 await menu.getByRole('button',{name:'Codex',exact:true}).click();
 await expect(menu.getByRole('status')).toHaveText('세션 1개 가져옴');expect(wire.calls).toHaveLength(2);
});

for(const imported of [0,1])test(`per-session errors are visible with ${imported} successful imports`,async({page,wire})=>{
 wire.reply.imported=imported;wire.reply.errors=[{session:'session-unavailable',error:'저장된 대화 파일을 읽을 수 없습니다.'}];
 const menu=await open(page);
 await menu.getByRole('button',{name:'Codex',exact:true}).click();
 await expect(menu.getByRole('status')).toHaveText(imported?'세션 1개 가져옴 · 1개 실패':'세션 1개 가져오기 실패');
 await menu.getByText('실패 원인 1개',{exact:true}).click();
 await expect(menu.getByText('저장된 대화 파일을 읽을 수 없습니다.',{exact:true})).toBeVisible();
 await expect(menu.getByText('session-unavailable',{exact:true})).toBeVisible();
 await expect(menu.getByRole('button',{name:'캔버스에서 보기'})).toHaveCount(imported?1:0);
});

test('connection failures retain the menu and retry controls',async({page,wire})=>{
 wire.error='Codex 응답 시간 초과';
 const menu=await open(page);
 await menu.getByRole('button',{name:'Codex',exact:true}).click();
 await expect(menu.getByRole('status')).toHaveText('Codex 가져오기 실패');
 await expect(menu.getByRole('alert')).toHaveText(wire.error);
 await expect(menu.getByRole('button',{name:'Codex',exact:true})).toBeEnabled();
 wire.error='';wire.reply.imported=0;
 await menu.getByRole('button',{name:'Codex',exact:true}).click();
 await expect(menu.getByRole('status')).toHaveText('가져올 외부 세션이 없습니다.');
 await expect(menu.getByRole('alert')).toHaveCount(0);
});

test('snapshot retry preserves a successful import without repeating discovery',async({page,wire})=>{
 wire.failRefresh=true;const menu=await open(page);
 await menu.getByRole('button',{name:'Codex',exact:true}).click();
 await expect(menu.getByRole('status')).toHaveText('세션 1개 가져옴');
 await expect(menu.getByRole('alert')).toContainText('목록 갱신 실패:');
 wire.failRefresh=false;
 await menu.getByRole('button',{name:'목록 새로고침',exact:true}).click();
 await expect(menu.getByRole('alert')).toHaveCount(0);
 await menu.getByRole('button',{name:'캔버스에서 보기',exact:true}).click();
 await expect(page.locator('[data-session-id="imported-session"]')).toBeInViewport();
 expect(wire.calls).toHaveLength(1);
});

test('late completion does not change the selected project or show another project result',async({page,wire})=>{
 let release!:()=>void;wire.wait=new Promise<void>(resolve=>release=resolve);
 const menu=await open(page,1280);
 try{
  await menu.getByRole('button',{name:'Codex',exact:true}).click();
  await expect(menu.getByRole('status')).toHaveText('Codex 세션 조회 중…');
  await showSidebar(page);await page.getByRole('combobox',{name:'프로젝트',exact:true}).selectOption('other-project');
 }finally{release();}
 await expect.poll(()=>wire.reads).toBeGreaterThan(1);
 await expect(page).toHaveURL(/project=other-project/);
 await expect(menu.getByRole('button',{name:'Codex',exact:true})).toBeEnabled();
 await expect(menu.getByRole('status')).toHaveCount(0);
 await expect(page.locator('[data-session-id="imported-session"]')).toHaveCount(0);
});

for(const colorScheme of ['light','dark'] as const)test(`import feedback and errors fit large mobile UI in ${colorScheme}`,async({page,wire})=>{
 await page.emulateMedia({colorScheme});
 wire.reply.errors=Array.from({length:8},(_,i)=>({session:'long-session-id-'+i+'x'.repeat(80),error:'긴 오류 메시지 '+('error-without-spaces-'.repeat(12))}));
 const menu=await open(page,390,2);
 await menu.getByRole('button',{name:'Codex',exact:true}).click();
 await expect(menu.getByRole('status')).toHaveText('세션 1개 가져옴 · 8개 실패');
 await menu.getByText('실패 원인 8개',{exact:true}).click();
 await menu.getByRole('button',{name:'캔버스에서 보기',exact:true}).scrollIntoViewIfNeeded();
 await expect(menu.getByRole('button',{name:'캔버스에서 보기',exact:true})).toBeInViewport();
 expect(await menu.evaluate(e=>e.scrollWidth-e.clientWidth)).toBeLessThanOrEqual(1);
 expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
 expect(await page.evaluate(()=>document.documentElement.scrollHeight-innerHeight)).toBeLessThanOrEqual(1);
 await expectOverlayScrolling(page);
});
