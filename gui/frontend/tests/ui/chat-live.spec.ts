import {test as base,expect,type Page} from '@playwright/test';
import type {Snapshot,Detail,Event,QueuedMessage,Submission,RunState} from '../../src/lib/types';
declare global{interface Window{bibiLiveEmit:(event:Event)=>void;bibiLiveReady:boolean;}}
type Wire={snapshot?:Snapshot;detail?:Detail;state:RunState;external:boolean;seq:number;reads:number;queue:QueuedMessage[];commands:Record<string,unknown>[];answer:string;hold?:Promise<void>;};
const test=base.extend<{wire:Wire}>({wire:[async({page,baseURL},use)=>{
 const errors:string[]=[],wire:Wire={state:'running',external:false,seq:1,reads:0,queue:[],commands:[],answer:'첫 문장.'};page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{class FixtureEvents extends EventTarget{onopen:(()=>void)|null=null;onerror:(()=>void)|null=null;constructor(){super();window.bibiLiveEmit=event=>this.dispatchEvent(new MessageEvent('update',{data:JSON.stringify(event)}));window.bibiLiveReady=true;queueMicrotask(()=>this.onopen?.());}close(){}}Object.defineProperty(window,'EventSource',{value:FixtureEvents});});
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());if(url.origin!==new URL(baseURL!).origin){errors.push('external request');return route.abort();}
  if(req.method()==='GET'){
   if(url.pathname==='/api/snapshot'){if(!wire.snapshot){wire.snapshot=await(await route.fetch()).json() as Snapshot;wire.snapshot.message_queue_v1=true;wire.snapshot.native_session_controls_v1=true;wire.snapshot.approval_modes_v1=true;wire.snapshot.last_seq=wire.seq;const run=wire.snapshot.runs.find(r=>r.id==='layout-parent')!;run.provider='claude';run.read_only=false;run.state=wire.state;run.phase='응답 대기';run.origin=wire.external?'external':'managed';run.approval_mode='accept_edits';wire.snapshot.providers[0].adapter='claude';}return route.fulfill({json:wire.snapshot});}
   if(url.pathname.startsWith('/api/runs/')){const run=wire.snapshot!.runs.find(r=>r.id==='layout-parent')!;if(url.pathname.endsWith('/status'))return route.fulfill({json:run});wire.reads++;if(!wire.detail){wire.detail=await(await route.fetch()).json() as Detail;wire.detail.run=run;wire.detail.messages=[{id:'u1',run_id:run.id,role:'user',text:'이전 질문 하나',created_at:1},{id:'u2',run_id:run.id,role:'user',text:'이전 질문 둘',created_at:2},{id:'answer',run_id:run.id,role:'assistant',text:wire.answer,created_at:3}];wire.detail.conversation=wire.detail.messages;}wire.detail.last_seq=wire.seq;wire.detail.queued_messages=wire.queue;const json=JSON.parse(JSON.stringify(wire.detail));await wire.hold;return route.fulfill({json});}
   return route.continue();
  }
  if(req.method()==='POST'&&url.pathname==='/api/command'){
   const body=req.postDataJSON();wire.commands.push(body);const run=wire.snapshot!.runs.find(r=>r.id==='layout-parent')!;
   if(body.type==='queue_message'){const q=body.request as Submission;wire.queue.push({id:q.submission_id,session_id:run.session_id,order:wire.queue.length+1,request:q,state:'queued',error:null,created_at:Date.now()});return route.fulfill({json:{submission_id:q.submission_id,run_id:run.id,request_id:run.request_id,work_id:run.work_id,status:'queued'}});}
   if(body.type==='change_queued_message'){wire.queue=wire.queue.filter(q=>q.id!==body.id);return route.fulfill({json:{}});}
   if(body.type==='open_session_window')return route.fulfill({json:{status:'attention',message:'Windows가 자동 포커스를 제한했습니다. 작업 표시줄에서 강조된 원래 터미널 창을 선택하세요.'}});
   if(body.type==='release_claude')return route.fulfill({json:{native_id:run.session_key,workspace:run.workspace,config_directory:'/fixture/claude',command:'claude --resume '+run.session_key}});
   errors.push('unexpected command '+body.type);return route.abort();
  }
  errors.push(req.method()+' '+url.pathname);return route.abort();
 });
 await use(wire);expect(errors).toEqual([]);
},{auto:true}]});
test.beforeEach(async({page},info)=>{const platform=info.project.metadata.platform as string|undefined;if(platform)await page.addInitScript(v=>Object.defineProperty(navigator,'platform',{value:v}),platform);});
async function open(page:Page,width=390){await page.setViewportSize({width,height:844});await page.goto('/?view=conversation&project=layout-project&run=layout-parent');await expect(page.locator('.message.assistant')).toBeVisible();await page.waitForFunction(()=>window.bibiLiveReady);}
async function emit(page:Page,wire:Wire,kind:string,data:unknown){const event={id:'e'+(++wire.seq),seq:wire.seq,kind,data,created_at:Date.now()} as Event;wire.snapshot!.last_seq=wire.seq;await page.evaluate(e=>window.bibiLiveEmit(e),event);}

test('active progress is visible inside chat and vanishes after completion',async({page,wire})=>{
 await open(page);const progress=page.getByRole('status',{name:'에이전트 진행 상태'});await expect(progress).toContainText('응답 대기');await expect(progress.locator('i')).toHaveCount(3);expect(await progress.locator('i').first().evaluate(el=>getComputedStyle(el).animationName)).toBe('none');
 const run=wire.snapshot!.runs.find(r=>r.id==='layout-parent')!;run.state='waiting_user';run.updated_at++;await emit(page,wire,'run',run);await expect(progress).toContainText('승인·입력 대기');run.state='completed';run.updated_at++;await emit(page,wire,'run',run);await expect(progress).toHaveCount(0);
});
test('streamed sentences preserve existing DOM and do not refetch full history',async({page,wire})=>{
 await open(page,1280);const p=page.locator('.message.assistant .markdown p').first();const handle=await p.elementHandle();const old=await page.locator('.message.user').first().elementHandle();const before=wire.reads;
 for(let i=0;i<25;i++){wire.detail!.messages[2].text+=' 다음'+i;await emit(page,wire,'output',{run_id:'layout-parent',message_id:'answer',role:'assistant',delta:' 다음'+i,created_at:3});await expect(p).toContainText('다음'+i);}
 expect(await handle!.evaluate(el=>el.isConnected)).toBe(true);expect(await old!.evaluate(el=>el.isConnected)).toBe(true);expect(wire.reads-before).toBeLessThanOrEqual(1);
 await emit(page,wire,'message',{...wire.detail!.messages[2],text:'최종 답변'});await expect(p).toHaveText('최종 답변');expect(await handle!.evaluate(el=>el.isConnected)).toBe(true);
});
test('queue accepts multiple questions, retains drafts in up/down history, and cancels individually',async({page,wire})=>{
 await open(page);const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('작성 중 초안');await input.press('Home');await input.press('ArrowUp');await expect(input).toHaveValue('이전 질문 둘');await input.press('ArrowUp');await expect(input).toHaveValue('이전 질문 하나');await input.press('End');await input.press('ArrowDown');await expect(input).toHaveValue('이전 질문 둘');await input.press('End');await input.press('ArrowDown');await expect(input).toHaveValue('작성 중 초안');
 await input.fill('대기 하나');await page.getByRole('button',{name:'대기열에 추가',exact:true}).click();await expect(page.locator('.queue-item')).toHaveCount(1);await expect(input).toBeFocused();await input.fill('대기 둘');await page.getByRole('button',{name:'대기열에 추가',exact:true}).click();await expect(page.locator('.queue-item')).toHaveCount(2);await expect(input).toHaveValue('');await input.press('ArrowUp');await expect(input).toHaveValue('대기 둘');expect(wire.commands.filter(c=>c.type==='submit')).toHaveLength(0);expect(wire.commands.filter(c=>c.type==='queue_message')).toHaveLength(2);await page.getByRole('button',{name:'대기 메시지 취소'}).first().click();await expect(page.locator('.queue-item')).toHaveCount(1);await expect(page.locator('.queue-item')).toContainText('대기 둘');
});
test('existing diagrams and table controls survive trailing streamed text',async({page,wire})=>{
 wire.answer='```mermaid\ngraph LR\n A-->B\n```\n\n|A|B|\n|--|--|\n|1|2|\n\n이후 문장';await open(page,1280);const diagram=page.getByRole('region',{name:'다이어그램 미리보기'});await expect(diagram.locator('img')).toBeVisible();const img=await diagram.locator('img').elementHandle();const copy=await page.getByRole('button',{name:'표 복사',exact:true}).elementHandle();for(let i=0;i<8;i++){wire.detail!.messages[2].text+=' 추가';await emit(page,wire,'output',{run_id:'layout-parent',message_id:'answer',role:'assistant',delta:' 추가',created_at:3});}await expect(page.locator('.message.assistant')).toContainText('이후 문장 추가 추가');expect(await img!.evaluate(el=>el.isConnected)).toBe(true);expect(await copy!.evaluate(el=>el.isConnected)).toBe(true);await expect(diagram.locator('img')).toBeVisible();
});
test('terminal actions use an unclipped dialog and the exact native identity',async({page,wire})=>{
 wire.state='completed';await open(page);await page.getByRole('button',{name:'원래 터미널',exact:true}).click();const dialog=page.getByRole('dialog',{name:'원래 터미널'});await expect(dialog).toBeVisible();const box=(await dialog.boundingBox())!;expect(box.x).toBeGreaterThanOrEqual(0);expect(box.x+box.width).toBeLessThanOrEqual(390);await dialog.getByRole('button',{name:'터미널로 돌려주기'}).click();await expect(dialog.getByLabel('실행 명령')).toHaveValue('claude --resume layout-parent');expect(wire.commands.find(c=>c.type==='release_claude')).toMatchObject({run_id:'layout-parent',expected_native_id:'layout-parent'});await dialog.getByRole('button',{name:'터미널 메뉴 닫기'}).click();await expect(dialog).toHaveCount(0);
});
test('external terminal focus reports OS focus restrictions truthfully',async({page,wire})=>{wire.external=true;await open(page,1280);await page.getByRole('button',{name:'원래 터미널',exact:true}).click();const dialog=page.getByRole('dialog',{name:'원래 터미널'});await dialog.getByRole('button',{name:'원래 창 열기'}).click();await expect(dialog.getByRole('status')).toContainText('자동 포커스를 제한');expect(wire.commands[0]).toMatchObject({type:'open_session_window',run_id:'layout-parent'});});

test('an older in-flight detail response replays new deltas without rolling back text',async({page,wire})=>{
 await open(page,1280);const before=wire.reads;let release!:()=>void;wire.hold=new Promise<void>(resolve=>release=resolve);const run=wire.snapshot!.runs.find(r=>r.id==='layout-parent')!;run.phase='답변 작성 중';run.updated_at++;await emit(page,wire,'run',run);await expect.poll(()=>wire.reads).toBeGreaterThan(before);
 const p=page.locator('.message.assistant .markdown p').first(),handle=await p.elementHandle();wire.detail!.messages[2].text+=' 늦은 응답';await emit(page,wire,'output',{run_id:'layout-parent',message_id:'answer',role:'assistant',delta:' 늦은 응답',created_at:3});await expect(p).toHaveText('첫 문장. 늦은 응답');wire.hold=undefined;release();await expect.poll(()=>page.locator('.message.assistant').innerText()).toContain('첫 문장. 늦은 응답');await emit(page,wire,'output',{run_id:'layout-parent',message_id:'answer',role:'assistant',delta:' 끝',created_at:3});await expect(p).toHaveText('첫 문장. 늦은 응답 끝');expect(await handle!.evaluate(el=>el.isConnected)).toBe(true);
});
