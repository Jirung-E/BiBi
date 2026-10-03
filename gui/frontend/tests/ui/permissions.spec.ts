import {test as base,expect,type Page} from '@playwright/test';
import type {Snapshot,Submission,Provider,RunState} from '../../src/lib/types';

type Wire={snapshot?:Snapshot;submissions:Submission[];provider:Provider;readOnly:boolean;state:RunState;status:number;statusReads:number};
const test=base.extend<{wire:Wire}>({wire:[async({page,baseURL},use)=>{
 const errors:string[]=[],wire:Wire={submissions:[],provider:'codex',readOnly:false,state:'completed',status:200,statusReads:0};
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());
  if(url.origin!==new URL(baseURL!).origin){errors.push('unexpected external request');return route.abort();}
  if(req.method()==='GET'){
   if(url.pathname==='/api/snapshot'){
    if(!wire.snapshot){
     wire.snapshot=await(await route.fetch()).json() as Snapshot;
     wire.snapshot.approval_modes_v1=true;
     for(const p of wire.snapshot.providers)p.adapter=wire.provider;
     for(const run of wire.snapshot.runs){run.provider=wire.provider;run.read_only=wire.readOnly;run.state=wire.state;run.approval_mode='on_request';}
    }
    return route.fulfill({json:wire.snapshot});
   }
   if(url.pathname.startsWith('/api/runs/')){
    const id=decodeURIComponent(url.pathname.split('/')[3]),run=wire.snapshot!.runs.find(r=>r.id===id)!;
    if(url.pathname.endsWith('/status')){await route.fulfill({json:run});wire.statusReads++;return;}
    if(id.startsWith('permission-turn-'))return route.fulfill({json:{run,messages:[],conversation:[],inbox:[],approvals:[],inputs:[]}});
    const detail=await(await route.fetch()).json();detail.run=run;return route.fulfill({json:detail});
   }
   return route.continue();
  }
  if(req.method()==='POST'&&url.pathname==='/api/command'){
   const body=req.postDataJSON();
   if(body.type==='select_model')return route.fulfill({json:{}});
   if(body.type==='submit'){
    const request=body.request as Submission;wire.submissions.push(request);
    if(wire.status!==200)return route.fulfill({status:wire.status,json:{error:'승인 모드 검증 오류'}});
    const source=wire.snapshot!.runs.find(r=>r.id===request.target_run_id)??wire.snapshot!.runs[0],id='permission-turn-'+wire.submissions.length;
    const run={...source,id,approval_mode:request.approval_mode??source.approval_mode,read_only:request.read_only,state:'completed' as const,continued_from:request.mode==='fresh'?null:source.id,session_id:request.mode==='fresh'?id:source.session_id,turn_id:id+'-native-turn'};
    wire.snapshot!.runs.push(run);
    return route.fulfill({json:{submission_id:request.submission_id,run_id:id,request_id:id+'-request',work_id:source.work_id,status:'accepted'}});
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
async function open(page:Page,width=390){
 await page.setViewportSize({width,height:844});await page.goto('/?view=conversation&project=layout-project&run=layout-parent');
 await expect(page.getByRole('textbox',{name:'메시지',exact:true})).toBeVisible();
}
test('approval selection is remembered as a draft and retained in the same conversation',async({page,wire})=>{
 await open(page);
 const mode=page.getByRole('combobox',{name:'승인 모드',exact:true});
 await expect(mode).toHaveValue('on_request');
 const model=page.getByLabel('모델',{exact:true}),history=page.getByRole('button',{name:'최근 모델',exact:true});
 const modelBox=(await model.boundingBox())!,historyBox=(await history.boundingBox())!,modeBox=(await mode.boundingBox())!;
 expect(Math.abs(modelBox.y+modelBox.height/2-historyBox.y-historyBox.height/2)).toBeLessThanOrEqual(1);
 for(const button of [page.getByRole('button',{name:'전송 설정',exact:true}),page.getByRole('button',{name:'전송',exact:true})]){
  const box=(await button.boundingBox())!;
  expect(Math.abs(box.y+box.height/2-modeBox.y-modeBox.height/2)).toBeLessThanOrEqual(1);
 }
 expect(modeBox.y).toBeGreaterThanOrEqual(modelBox.y+modelBox.height);
 await mode.selectOption('full_access');
 await expect(page.getByRole('status')).toContainText('다음 메시지부터 적용');
 expect(wire.submissions).toHaveLength(0);
 await page.reload();await expect(mode).toHaveValue('full_access');
 await page.getByRole('textbox',{name:'메시지',exact:true}).fill('선택한 모드로 이어가기');
 await page.getByRole('button',{name:'전송',exact:true}).click();
 await expect(page).toHaveURL(/run=permission-turn-1/);await expect(mode).toHaveValue('full_access');
 expect(wire.submissions[0]).toMatchObject({approval_mode:'full_access',mode:'continue',target_run_id:'layout-parent'});
 await mode.selectOption('on_request');
 await page.getByRole('textbox',{name:'메시지',exact:true}).fill('승인 다시 요청');
 await page.getByRole('button',{name:'전송',exact:true}).click();
 await expect(page).toHaveURL(/run=permission-turn-2/);await expect(mode).toHaveValue('on_request');
 expect(wire.snapshot!.runs.at(-1)!.session_id).toBe('layout-parent');
 expect(wire.submissions[1].approval_mode).toBe('on_request');
});
test('Claude exposes edit approval and failed submissions keep the choice',async({page,wire})=>{
 wire.provider='claude';wire.status=409;await open(page,1280);
 const mode=page.getByRole('combobox',{name:'승인 모드',exact:true});await mode.selectOption('accept_edits');
 await page.getByRole('textbox',{name:'메시지',exact:true}).fill('편집 승인 모드');await page.getByRole('button',{name:'전송',exact:true}).click();
 await expect(page.getByRole('alert')).toHaveText('승인 모드 검증 오류');await expect(mode).toHaveValue('accept_edits');
 expect(wire.submissions[0].approval_mode).toBe('accept_edits');
});
test('new work defaults to approvals and sends an explicit mode',async({page,wire})=>{
 // Start on the page being tested; a redundant hard navigation can abort the
 // fixture's open EventSource in WebKit before the approval interaction starts.
 await page.setViewportSize({width:390,height:844});await page.goto('/?view=canvas&project=layout-project');
 await page.getByRole('button',{name:'새 업무',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'새 업무',exact:true}),mode=dialog.getByRole('combobox',{name:'승인 모드',exact:true});
 await expect(mode).toHaveValue('on_request');await expect(mode.locator('option')).toHaveCount(2);
 await mode.selectOption('full_access');await dialog.getByRole('textbox',{name:'메시지',exact:true}).fill('새 업무 승인 모드');
 await dialog.getByRole('button',{name:'전송',exact:true}).click();await expect(dialog).toHaveCount(0);
 expect(wire.submissions[0]).toMatchObject({approval_mode:'full_access',mode:'fresh',target_run_id:null});
});
test('read-only sessions cannot grant approval privileges',async({page,wire})=>{
 wire.readOnly=true;await open(page);
 const mode=page.getByRole('combobox',{name:'승인 모드',exact:true});await expect(mode).toBeDisabled();
 await expect(mode).toHaveText('읽기 전용');expect(wire.submissions).toHaveLength(0);
});
test('unsupported providers do not offer a pretend approval setting',async({page,wire})=>{
 wire.provider='ollama';wire.readOnly=true;await open(page);
 await expect(page.getByRole('combobox',{name:'승인 모드',exact:true})).toHaveCount(0);
});
test('steering a running turn cannot change its approval mode',async({page,wire})=>{
 wire.state='running';await open(page);
 // Exercise the periodic refresh before interacting; otherwise a fast local
 // run hides inconsistent fixture data that slower CI receives mid-action.
 await expect.poll(()=>wire.statusReads).toBeGreaterThan(0);
 await page.getByRole('button',{name:'전송 설정',exact:true}).click();
 await expect(page.getByRole('combobox',{name:'전송 방식',exact:true}).locator('option[value="steer"]')).toHaveCount(1);
 await page.getByRole('combobox',{name:'전송 방식',exact:true}).selectOption('steer');
 await expect(page.getByRole('combobox',{name:'승인 모드',exact:true})).toBeDisabled();
 expect(wire.submissions).toHaveLength(0);
});
for(const width of [320,390])for(const theme of ['light','dark'] as const)test(`approval controls fit ${width}px ${theme} at 200%`,async({page,wire})=>{
 wire.provider='claude';await page.emulateMedia({colorScheme:theme});
 await page.addInitScript(()=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:2,halfLife:30,floor:.15})));
 await open(page,width);const mode=page.getByRole('combobox',{name:'승인 모드',exact:true});
 await mode.selectOption('accept_edits');await mode.scrollIntoViewIfNeeded();await expect(mode).toBeInViewport();
 for(const control of [mode,page.getByRole('button',{name:'전송',exact:true})]){
  await control.scrollIntoViewIfNeeded();await expect(control).toBeInViewport();
  const box=(await control.boundingBox())!;expect(box.x).toBeGreaterThanOrEqual(0);expect(box.x+box.width).toBeLessThanOrEqual(width+1);
 }
 const overflow=await page.evaluate(()=>({width:document.documentElement.scrollWidth,height:document.documentElement.scrollHeight,viewport:innerHeight}));
 expect(overflow.width).toBeLessThanOrEqual(width+1);expect(overflow.height).toBeLessThanOrEqual(overflow.viewport+1);
});

test('an old connected server cannot silently ignore a selected mode',async({page,wire})=>{
 await open(page);wire.snapshot!.approval_modes_v1=false;
 await page.reload();const mode=page.getByRole('combobox',{name:'승인 모드',exact:true});
 await expect(mode).toBeDisabled();await expect(mode).toHaveAttribute('title',/서버를 업데이트/);
 expect(wire.submissions).toHaveLength(0);
});

for(const width of [390,1280])for(const theme of ['light','dark'] as const)test(`composer keeps its height while busy or changing approvals at ${width}px ${theme}`,async({page,wire})=>{
 wire.provider='claude';await page.emulateMedia({colorScheme:theme});await open(page,width);
 const composer=page.getByRole('form',{name:'메시지 작성',exact:true}),input=page.getByRole('textbox',{name:'메시지',exact:true});
 const mode=page.getByRole('combobox',{name:'승인 모드',exact:true}),send=page.getByRole('button',{name:'전송',exact:true});
 await input.fill('다음 질문 초안');await expect(send).toBeEnabled();
 const height=(await composer.boundingBox())!.height;
 await mode.selectOption('full_access');
 await expect(mode).toHaveAccessibleDescription(/다음 메시지부터 적용.*전체 접근/);
 await expect(mode).toHaveAttribute('title',/전체 접근/);
 expect((await composer.boundingBox())!.height).toBeCloseTo(height,1);
 for(const run of wire.snapshot!.runs){run.state='running';run.phase='실행 중';}
 await page.reload();await expect(input).toHaveValue('다음 질문 초안');await expect(input).toBeEditable();await expect(send).toBeDisabled();
 await expect(input).toHaveAccessibleDescription('현재 응답이 끝나면 이어서 보낼 수 있습니다.');
 await expect(send).toHaveAccessibleDescription('현재 응답이 끝나면 이어서 보낼 수 있습니다.');
 await expect(send).toHaveAttribute('title','현재 응답이 끝나면 이어서 보낼 수 있습니다.');
 expect((await composer.boundingBox())!.height).toBeCloseTo(height,1);
 const helperBox=(await composer.locator('[id$="-waiting"]').boundingBox())!;
 expect(helperBox.height).toBeLessThanOrEqual(1);
 await expect(composer.locator(':scope > small')).toHaveCount(0);
 await input.fill('/bibi usage');await expect(send).toBeEnabled();
 await expect(send).not.toHaveAttribute('aria-describedby');
 expect(wire.submissions).toHaveLength(0);
});
test('recent models and send settings occupy a single auxiliary area',async({page})=>{
 await open(page);const history=page.getByRole('button',{name:'최근 모델',exact:true}),settings=page.getByRole('button',{name:'전송 설정',exact:true});
 await settings.click();await expect(settings).toHaveAttribute('aria-expanded','true');
 await history.click();await expect(history).toHaveAttribute('aria-expanded','true');
 await expect(settings).toHaveAttribute('aria-expanded','false');await expect(page.locator('.composer-settings')).toHaveCount(0);
 await expect(page.locator('.model-history')).toBeVisible();
 await settings.click();await expect(settings).toHaveAttribute('aria-expanded','true');
 await expect(history).toHaveAttribute('aria-expanded','false');await expect(page.locator('.model-history')).toHaveCount(0);
 await settings.click();await expect(page.locator('.composer-settings,.model-history')).toHaveCount(0);
});
test('new session execution options fold into send settings without losing values',async({page})=>{
 // Start on the page being tested; a redundant hard navigation can abort the
 // fixture's open EventSource in WebKit before the approval interaction starts.
 await page.setViewportSize({width:390,height:844});await page.goto('/?view=canvas&project=layout-project');await page.getByRole('button',{name:'새 업무',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'새 업무',exact:true}),settings=dialog.getByRole('button',{name:'전송 설정',exact:true});
 await dialog.getByText('실행 옵션',{exact:true}).click();await dialog.getByRole('textbox',{name:'역할',exact:true}).fill('검토 담당');
 await settings.click();await expect(dialog.locator('.execution-options')).toHaveCount(0);
 await expect(dialog.getByRole('combobox',{name:'승인 모드',exact:true})).toBeVisible();
 await settings.click();await dialog.getByText('실행 옵션',{exact:true}).click();
 await expect(dialog.getByRole('textbox',{name:'역할',exact:true})).toHaveValue('검토 담당');
});
