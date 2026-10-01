import {test as base,expect,type Page} from '@playwright/test';
import type {Snapshot} from '../../src/lib/types';
import {sidebarAction} from './navigation';
import {expectOverlayScrolling} from './scrolling';

type Cleanup={type:string;project_key:string;run_ids:string[];run_id?:string;hidden?:boolean};
type Wire={snapshot?:Snapshot;calls:Cleanup[];wait:Promise<void>|null;error:string;failRefresh:boolean;completed:boolean;skip:string[];extra:number;old:boolean};
const test=base.extend<{wire:Wire}>({wire:[async({page,baseURL},use)=>{
 const errors:string[]=[],wire:Wire={calls:[],wait:null,error:'',failRefresh:false,completed:false,skip:[],extra:0,old:false};
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());
  if(url.origin!==new URL(baseURL!).origin){errors.push('external request');return route.abort();}
  if(req.method()==='GET'){
   if(url.pathname==='/api/snapshot'){
    if(wire.completed&&wire.failRefresh)return route.fulfill({status:503,json:{error:'목록 응답 시간 초과'}});
    if(!wire.snapshot){
     const s=await(await route.fetch()).json() as Snapshot;wire.snapshot=s;s.session_cleanup_v1=!wire.old;s.projects[0].name='정리 검증 프로젝트';s.providers[0].name='검증 제공자';
     s.runs[0].title='연결 끊긴 대화';s.runs[0].state='disconnected';s.runs[0].continued_from='previous-turn';
     s.runs[1].title='원격 서브에이전트';s.runs[1].state='disconnected';s.runs[1].host_id='other-host';
     s.runs[2].state='running';s.runs[2].title='진행 중인 세션';
     s.runs[3].state='disconnected';s.runs[3].title='승인 대기 보호';s.approvals=[{id:'pending',run_id:s.runs[3].id,native_id:1,kind:'fixture',title:'대기',detail:{},state:'pending',created_at:0}];
     s.runs.push({...s.runs[0],id:'previous-turn',state:'completed',continued_from:null,created_at:s.runs[0].created_at-1});
     s.projects.push({...s.projects[0],id:'other-project',name:'다른 프로젝트'});
     s.runs.push({...s.runs[0],id:'other-project-run',session_id:'other-project-run',project_key:'other-project',title:'다른 프로젝트 세션'});
     for(let i=0;i<wire.extra;i++)s.runs.push({...s.runs[1],id:'extra-'+i,session_id:'extra-'+i,title:'연결 끊긴 세션 '+i+' 긴이름'.repeat(20)});
    }
    return route.fulfill({json:wire.snapshot});
   }
   if(url.pathname.startsWith('/api/runs/')){
    const id=url.pathname.split('/').at(-1),run=[...wire.snapshot!.runs,...wire.snapshot!.removed_sessions].find(r=>r.id===id);
    return route.fulfill({json:{run,messages:[],conversation:[],inbox:[],approvals:[],inputs:[]}});
   }
   return route.continue();
  }
  if(req.method()==='POST'&&url.pathname==='/api/command'){
   const body=req.postDataJSON() as Cleanup,s=wire.snapshot!;wire.calls.push(body);
   if(body.type==='cleanup_disconnected_sessions'){
    await wire.wait;if(wire.error)return route.fulfill({status:503,json:{error:wire.error}});
    const hidden:string[]=[],skipped:string[]=[];
    for(const id of body.run_ids){const run=s.runs.find(r=>r.id===id);if(!run||wire.skip.includes(id)){skipped.push(id);if(run)run.state='running';}else hidden.push(run.session_id);}
    s.removed_sessions.push(...s.runs.filter(r=>hidden.includes(r.session_id)));s.runs=s.runs.filter(r=>!hidden.includes(r.session_id));s.last_seq++;wire.completed=true;
    return route.fulfill({json:{hidden_session_ids:hidden,skipped_run_ids:skipped}});
   }
   if(body.type==='set_session_hidden'&&body.hidden===false){
    const session=s.removed_sessions.find(r=>r.id===body.run_id)?.session_id;
    s.runs.push(...s.removed_sessions.filter(r=>r.session_id===session));s.removed_sessions=s.removed_sessions.filter(r=>r.session_id!==session);s.last_seq++;
    return route.fulfill({json:{saved:true}});
   }
  }
  errors.push(`unexpected ${req.method()} ${url.pathname}`);return route.abort();
 });
 await use(wire);expect(errors).toEqual([]);
},{auto:true}]});
test.beforeEach(async({page},info)=>{
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
});
async function canvasCleanup(page:Page){
 await page.getByRole('button',{name:'캔버스 메뉴',exact:true}).click();
 await page.getByRole('button',{name:'연결 끊긴 세션 정리',exact:true}).click();
}
async function open(page:Page,width=390,scale=1,view='canvas'){
 await page.setViewportSize({width,height:844});await page.addInitScript(value=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:value})),scale);
 await page.goto('/?view='+view+'&project=layout-project'+(view==='conversation'?'&run=layout-parent':''));
 if(view!=='canvas')await sidebarAction(page,'세션 캔버스');
 await canvasCleanup(page);return page.getByRole('dialog',{name:'연결 끊긴 세션 정리',exact:true});
}
for(const width of [390,1280])test(`cleanup previews unique disconnected sessions only, confirms and restores at ${width}px`,async({page,wire})=>{
 const dialog=await open(page,width);
 await expect(dialog.getByRole('checkbox')).toHaveCount(3);await expect(dialog).not.toContainText('진행 중인 세션');await expect(dialog).not.toContainText('다른 프로젝트 세션');await expect(dialog).not.toContainText('승인 대기 보호');
 expect(wire.calls).toHaveLength(0);await dialog.getByRole('checkbox',{name:'원격 서브에이전트',exact:true}).uncheck();
 await dialog.getByRole('button',{name:'1개 정리',exact:true}).click();await expect(dialog.getByRole('status')).toHaveText('1개 정리됨');
 expect(wire.calls[0]).toEqual({type:'cleanup_disconnected_sessions',project_key:'layout-project',run_ids:['layout-parent']});
 expect(wire.snapshot!.removed_sessions).toHaveLength(2);await expect(dialog.getByRole('checkbox',{name:'연결 끊긴 대화',exact:true})).toHaveCount(0);
 await dialog.getByRole('button',{name:'제거한 세션 보기',exact:true}).click();
 const settings=page.getByRole('dialog',{name:'설정',exact:true});await expect(settings.locator('.removed-sessions')).toHaveAttribute('open','');
 const removedToggle=settings.getByText('제거한 세션 · 1',{exact:true});
 await removedToggle.click();await expect(settings.locator('.removed-sessions')).not.toHaveAttribute('open','');
 await removedToggle.click();await settings.getByRole('button',{name:'복원',exact:true}).click();
 await expect(settings.getByText('제거한 세션 · 1',{exact:true})).toHaveCount(0);expect(wire.snapshot!.removed_sessions).toHaveLength(0);expect(wire.snapshot!.runs.filter(r=>r.session_id==='layout-parent')).toHaveLength(2);
});
test('cancel and browser Back do not mutate sessions',async({page,wire})=>{
 const dialog=await open(page);await page.goBack();await expect(dialog).toHaveCount(0);expect(wire.calls).toHaveLength(0);
 await canvasCleanup(page);await dialog.getByRole('button',{name:'취소',exact:true}).click();await expect(dialog).toHaveCount(0);expect(wire.calls).toHaveLength(0);
});
test('all selection and empty selection behave without an accidental command',async({page,wire})=>{
 const dialog=await open(page);await dialog.getByRole('checkbox',{name:'전체 선택',exact:false}).uncheck();await expect(dialog.getByRole('button',{name:'0개 정리'})).toBeDisabled();expect(wire.calls).toHaveLength(0);
 await dialog.getByRole('checkbox',{name:'전체 선택',exact:false}).check();await expect(dialog.getByRole('button',{name:'2개 정리'})).toBeEnabled();
});
test('pending cleanup prevents duplicates and reports recovered sessions separately',async({page,wire})=>{
 let release!:()=>void;wire.wait=new Promise<void>(resolve=>release=resolve);wire.skip=['layout-child'];const dialog=await open(page);
 try{await dialog.getByRole('button',{name:'2개 정리'}).click();await expect(dialog.getByRole('button',{name:'정리 중…'})).toBeDisabled();await expect(dialog.getByRole('checkbox',{name:'연결 끊긴 대화',exact:true})).toBeDisabled();expect(wire.calls).toHaveLength(1);}finally{release();}
 await expect(dialog.getByRole('status')).toHaveText('1개 정리됨 · 1개 제외');expect(wire.snapshot!.runs.find(r=>r.id==='layout-child')!.state).toBe('running');
});
test('command failure keeps the selection for an explicit retry',async({page,wire})=>{
 wire.error='서버 연결 실패';const dialog=await open(page);await dialog.getByRole('button',{name:'2개 정리'}).click();await expect(dialog.getByRole('alert')).toHaveText(wire.error);
 await expect(dialog.getByRole('checkbox',{name:'연결 끊긴 대화',exact:true})).toBeChecked();wire.error='';await dialog.getByRole('button',{name:'2개 정리'}).click();await expect(dialog.getByRole('status')).toHaveText('2개 정리됨');expect(wire.calls).toHaveLength(2);
});
test('successful cleanup with failed refresh retries only the read',async({page,wire})=>{
 wire.failRefresh=true;const dialog=await open(page);await dialog.getByRole('button',{name:'2개 정리'}).click();await expect(dialog.getByRole('status')).toHaveText('2개 정리됨');await expect(dialog.getByRole('alert')).toContainText('목록 갱신 실패');
 wire.failRefresh=false;await dialog.getByRole('button',{name:'목록 새로고침'}).click();await expect(dialog.getByRole('alert')).toHaveCount(0);await expect(dialog.getByRole('checkbox')).toHaveCount(0);expect(wire.calls).toHaveLength(1);
});
test('cleaning the open conversation clears it even when the modal closes through history',async({page})=>{
 const dialog=await open(page,390,1,'conversation');await dialog.getByRole('button',{name:'2개 정리'}).click();await expect(dialog.getByRole('status')).toHaveText('2개 정리됨');
 await dialog.getByRole('button',{name:'닫기',exact:true}).last().click();await expect(page).not.toHaveURL(/run=/);await expect(page.locator('.board')).toBeVisible();
});
test('old servers expose the need to update without sending an unknown command',async({page,wire})=>{
 wire.old=true;const dialog=await open(page);await expect(dialog.getByRole('alert')).toContainText('서버를 업데이트');await expect(dialog.getByRole('button',{name:'2개 정리'})).toBeDisabled();expect(wire.calls).toHaveLength(0);
});
test('empty cleanup deep links and project switching keep their scope',async({page,wire})=>{
 await open(page);await page.goto('/?view=canvas&project=other-project&modal=cleanup');let dialog=page.getByRole('dialog',{name:'연결 끊긴 세션 정리',exact:true});
 await expect(dialog.getByRole('checkbox',{name:'다른 프로젝트 세션',exact:true})).toBeVisible();await dialog.getByRole('button',{name:'1개 정리'}).click();await expect(dialog.getByRole('status')).toHaveText('1개 정리됨');expect(wire.calls[0].project_key).toBe('other-project');
 await page.reload();dialog=page.getByRole('dialog',{name:'연결 끊긴 세션 정리',exact:true});await expect(dialog.getByRole('status')).toHaveText('정리할 연결 끊긴 세션이 없습니다.');
});
for(const theme of ['light','dark'] as const)test(`long cleanup lists fit 320px UI 200% in ${theme}`,async({page,wire})=>{
 wire.extra=20;await page.emulateMedia({colorScheme:theme});const dialog=await open(page,320,2);
 await expect(dialog).toBeVisible();await expect(dialog.getByRole('checkbox')).toHaveCount(23);
 const confirm=dialog.getByRole('button',{name:'22개 정리'});await confirm.scrollIntoViewIfNeeded();await expect(confirm).toBeInViewport();
 expect(await page.evaluate(()=>Math.max(document.documentElement.scrollWidth-innerWidth,document.body.scrollWidth-innerWidth))).toBeLessThanOrEqual(1);
 expect(await dialog.evaluate(e=>e.scrollWidth-e.clientWidth)).toBeLessThanOrEqual(1);await expectOverlayScrolling(page);
 expect(wire.calls).toHaveLength(0);
});

test('cleanup is scoped to the canvas menu, with a keyboard and outside-click exit',async({page})=>{
 await page.setViewportSize({width:1280,height:900});await page.goto('/?view=conversation&project=layout-project&run=layout-parent');
 await expect(page.getByRole('button',{name:'연결 끊긴 세션 정리',exact:true})).toHaveCount(0);
 await expect(page.getByRole('button',{name:'캔버스 메뉴',exact:true})).toHaveCount(0);
 await sidebarAction(page,'세션 캔버스');
 const trigger=page.getByRole('button',{name:'캔버스 메뉴',exact:true});
 await trigger.press('Enter');
 const cleanup=page.getByRole('button',{name:'연결 끊긴 세션 정리',exact:true});await expect(cleanup).toBeFocused();
 await page.keyboard.press('Escape');await expect(trigger).toBeFocused();await expect(cleanup).toHaveCount(0);
 await trigger.click();await expect(cleanup).toBeVisible();await page.locator('.toolbar-title').click();await expect(cleanup).toHaveCount(0);
 await sidebarAction(page,'사용량·연결');await expect(trigger).toHaveCount(0);
 await expect(page.getByRole('button',{name:'연결 끊긴 세션 정리',exact:true})).toHaveCount(0);
});
