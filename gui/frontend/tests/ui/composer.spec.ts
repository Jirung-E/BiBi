import {test as base,expect,type Page} from '@playwright/test';
import type {Snapshot,Submission,RunState,Event} from '../../src/lib/types';

type Wire={snapshot?:Snapshot;submissions:Submission[];wait:Promise<void>|null;status:number;state:RunState;phase:string|null;error:string|null;stream:Promise<Event>|null;snapshotWait:Promise<void>|null;snapshotWaiting:boolean;detailState:RunState|null};
const test=base.extend<{wire:Wire}>({
 wire:[async({page,baseURL},use)=>{
  const errors:string[]=[],wire:Wire={submissions:[],wait:null,status:200,state:'completed',phase:null,error:null,stream:null,snapshotWait:null,snapshotWaiting:false,detailState:null};
  page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/*',async route=>{
   const request=route.request(),url=new URL(request.url());
   if(url.origin===new URL(baseURL!).origin){
    if(request.method()==='GET'){
     if(url.pathname==='/api/stream'&&wire.stream){
      const event=await wire.stream;
      return route.fulfill({contentType:'text/event-stream',body:`event: update\ndata: ${JSON.stringify(event)}\n\n`});
     }
     if(url.pathname==='/api/snapshot'){
      wire.snapshot??=await (await route.fetch()).json() as Snapshot;
      const snapshot=structuredClone(wire.snapshot);wire.snapshotWaiting=!!wire.snapshotWait;
      await wire.snapshotWait;await route.fulfill({json:snapshot});wire.snapshotWaiting=false;return;
     }
     if(url.pathname.startsWith('/api/runs/focus-turn-')){
      const saved=wire.snapshot!.runs.find(r=>r.id===url.pathname.split('/')[3])!;
      const run=wire.detailState?{...saved,state:wire.detailState,phase:'결과 저장됨',updated_at:saved.updated_at+1}:saved;
      return route.fulfill({json:url.pathname.endsWith('/status')?run:{run,messages:[],conversation:[],inbox:[],approvals:[],inputs:[]}});
     }
     return route.continue();
    }
    if(request.method()==='POST'&&url.pathname==='/api/command'){
     const body=request.postDataJSON();
     if(body.type==='select_model'){
      wire.snapshot!.model_selection=body.selection;return route.fulfill({json:{}});
     }
     if(body.type==='submit'){
      const submission=body.request as Submission;
      expect(submission.provider).toBe('mock');
      wire.submissions.push(submission);await wire.wait;
      if(wire.status!==200)return route.fulfill({status:wire.status,json:{error:'검증용 전송 오류'}});
      const snapshot=wire.snapshot!,source=snapshot.runs.find(r=>r.id===submission.target_run_id)??snapshot.runs[0];
      const id='focus-turn-'+wire.submissions.length,workId=submission.work_id??'focus-work';
      const run={...source,id,model:submission.model,session_id:submission.mode==='fresh'?id:source.session_id,continued_from:submission.mode==='fresh'?null:source.id,work_id:workId,turn_id:id+'-turn',state:wire.state,phase:wire.phase??source.phase,error:wire.error};
      if(!snapshot.works.some(w=>w.id===workId))snapshot.works.push({...snapshot.works[0],id:workId});
      snapshot.runs.push(run);snapshot.last_seq++;
      return route.fulfill({json:{submission_id:submission.submission_id,run_id:id,request_id:id+'-request',work_id:workId,status:'accepted'}});
     }
    }
   }
   errors.push(`unexpected request: ${request.method()} ${request.url()}`);return route.abort();
  });
  await use(wire);expect(errors).toEqual([]);
 },{auto:true}]
});
const conversation='/?view=conversation&project=layout-project&run=layout-parent';
async function open(page:Page,width=1280){
 await page.setViewportSize({width,height:844});await page.goto(conversation);
 await expect(page.locator('.conversation-panel .composer textarea')).toBeVisible();
}
async function submit(page:Page,method:'keyboard'|'button'){
 if(method==='keyboard')await page.keyboard.press('Control+Enter');
 else await page.getByRole('button',{name:'전송',exact:true}).click();
}
for(const width of [1280,390])for(const method of ['keyboard','button'] as const){
 test(`composer keeps focus after ${method} submission at ${width}px`,async({page,wire})=>{
  await open(page,width);const input=page.getByRole('textbox',{name:'메시지',exact:true});
  await input.fill('첫 질문');await submit(page,method);
  await expect(page).toHaveURL(/run=focus-turn-1/);
  await expect(input).toBeEditable();await expect(input).toHaveValue('');await expect(input).toBeFocused();
  await page.keyboard.insertText('클릭 없이 이어 쓰는 질문');
  await expect(input).toHaveValue('클릭 없이 이어 쓰는 질문');await submit(page,method);
  await expect(page).toHaveURL(/run=focus-turn-2/);await expect(input).toBeFocused();
  expect(wire.submissions.map(s=>s.question)).toEqual(['첫 질문','클릭 없이 이어 쓰는 질문']);
  expect(wire.submissions[1].target_run_id).toBe('focus-turn-1');
 });
}
test('pending submission keeps focus and prevents edits and duplicate sends',async({page,wire})=>{
 await open(page);const input=page.getByRole('textbox',{name:'메시지',exact:true});
 let release!:()=>void;wire.wait=new Promise<void>(resolve=>release=resolve);wire.state='running';
 try{
  await input.fill('접수 중 질문');await submit(page,'keyboard');
  await expect.poll(()=>wire.submissions.length).toBe(1);
  await expect(input).toBeFocused();await expect(input).not.toBeEditable();
  await page.keyboard.insertText('덮어쓰지 않음');await submit(page,'keyboard');
  await expect(input).toHaveValue('접수 중 질문');expect(wire.submissions).toHaveLength(1);
 }finally{release();}
 await expect(page).toHaveURL(/run=focus-turn-1/);await expect(input).toBeEditable();await expect(input).toBeFocused();
 await page.keyboard.insertText('응답 중 준비한 다음 질문');
 await expect(input).toHaveValue('응답 중 준비한 다음 질문');
 await expect(page.getByRole('button',{name:'전송',exact:true})).toBeDisabled();
 await submit(page,'keyboard');await page.evaluate(()=>new Promise(requestAnimationFrame));
 expect(wire.submissions).toHaveLength(1);
});
test('rejected submission keeps the draft focused for correction',async({page,wire})=>{
 await open(page);wire.status=409;
 const input=page.getByRole('textbox',{name:'메시지',exact:true});
 await input.fill('수정할 질문');await submit(page,'button');
 await expect(page.getByRole('alert')).toHaveText('검증용 전송 오류');
 await expect(input).toBeEditable();await expect(input).toBeFocused();
 await page.keyboard.insertText(' 추가');await expect(input).toHaveValue('수정할 질문 추가');
 wire.status=200;await submit(page,'keyboard');await expect(page).toHaveURL(/run=focus-turn-2/);
 await expect(input).toBeFocused();
});
test('acceptance does not reclaim focus after the user selects another control',async({page,wire})=>{
 await open(page);const input=page.getByRole('textbox',{name:'메시지',exact:true});
 let release!:()=>void;wire.wait=new Promise<void>(resolve=>release=resolve);
 const context=page.getByRole('button',{name:'업무 맥락',exact:true});
 try{
  await input.fill('다른 조작도 가능한 질문');await submit(page,'keyboard');
  await expect.poll(()=>wire.submissions.length).toBe(1);await context.click();await context.focus();
 }finally{release();}
 await expect(page).toHaveURL(/run=focus-turn-1/);await expect(context).toBeFocused();
});
test('a new work transfers input focus from its dialog into the conversation',async({page})=>{
 await page.setViewportSize({width:390,height:844});await page.goto('/?view=canvas&project=layout-project');
 await page.getByRole('button',{name:'새 업무',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'새 업무',exact:true});
 await dialog.getByRole('textbox',{name:'메시지',exact:true}).fill('새 업무의 첫 질문');
 await dialog.getByRole('button',{name:'전송',exact:true}).click();
 await expect(dialog).toHaveCount(0);await expect(page).toHaveURL(/run=focus-turn-1/);
 const input=page.locator('.conversation-panel .composer textarea');
 await expect(input).toBeEditable();await expect(input).toBeFocused();
 await page.keyboard.insertText('새 대화에서 바로 이어 쓰기');
 await expect(input).toHaveValue('새 대화에서 바로 이어 쓰기');
});

for(const width of [390,1280])test(`model changes keep the same chat and selected model at ${width}px`,async({page,wire})=>{
 await open(page,width);
 const input=page.getByRole('textbox',{name:'메시지',exact:true}),model=page.getByLabel('모델',{exact:true});
 await model.fill('another-local-model');await input.fill('새 모델로 이어가기');
 // Preferences refresh must not replace an edited model with the old run model.
 await page.getByRole('button',{name:'업무 맥락',exact:true}).click();
 await expect(model).toHaveValue('another-local-model');
 await submit(page,'button');await expect(page).toHaveURL(/run=focus-turn-1/);
 expect(wire.submissions[0]).toMatchObject({mode:'continue',target_run_id:'layout-parent',model:'another-local-model'});
 expect(wire.snapshot!.runs.at(-1)!.session_id).toBe('layout-parent');
 await expect(model).toHaveValue('another-local-model');await expect(input).toBeFocused();
 await expect(page.locator('.conversation-heading small')).toContainText('another-local-model');
 await page.getByRole('button',{name:'최근 모델',exact:true}).click();
 const recent=page.locator('.model-history button').last(),chosen=await recent.innerText();
 await recent.click();await expect(model).toHaveValue(chosen);await expect(input).toBeFocused();
 await page.keyboard.insertText('최근 모델로 다시 이어가기');await submit(page,'keyboard');
 await expect(page).toHaveURL(/run=focus-turn-2/);
 expect(wire.submissions[1]).toMatchObject({mode:'continue',target_run_id:'focus-turn-1',model:chosen});
 expect(wire.snapshot!.runs.at(-1)!.session_id).toBe('layout-parent');
});

for(const width of [390,1280])test(`thinking and empty-answer errors update live and keep the chat usable at ${width}px`,async({page,wire})=>{
 let emit!:(event:Event)=>void;wire.stream=new Promise(resolve=>emit=resolve);
 wire.state='running';wire.phase='생각 중';
 await open(page,width);
 const input=page.getByRole('textbox',{name:'메시지',exact:true});
 await input.fill('응답 확인');await submit(page,'keyboard');
 await expect(page).toHaveURL(/run=focus-turn-1/);
 const badge=page.locator('.conversation-heading .badge');
 await expect(badge).toHaveText('생각 중');
 const run=wire.snapshot!.runs.at(-1)!;
 run.state='failed';run.phase='실패';run.error='Ollama가 추론만 보내고 최종 답변 없이 종료했습니다. 같은 대화에서 다시 질문하거나 모델을 변경해 주세요.';
 emit({seq:++wire.snapshot!.last_seq,id:'empty-answer-event',kind:'run',data:run,created_at:Date.now()});
 await expect(badge).toHaveText('실패');
 await expect(page.locator('.messages .error')).toHaveText(run.error);
 await expect(page.locator('.messages .message.assistant')).toHaveCount(0);
 await expect(page.getByRole('button',{name:'전송',exact:true})).toBeDisabled();
 await expect(input).toBeEditable();
 wire.state='completed';wire.phase='결과 저장됨';wire.error=null;
 await input.fill('같은 대화에서 다시 답해줘');await submit(page,'keyboard');
 await expect(page).toHaveURL(/run=focus-turn-2/);
 await expect(badge).toHaveText('결과 저장됨');
 await expect(page.locator('.messages .error')).toHaveCount(0);
 expect(wire.submissions[1]).toMatchObject({mode:'continue',target_run_id:'focus-turn-1'});
 expect(wire.snapshot!.runs.at(-1)!.session_id).toBe('layout-parent');
});


for(const width of [390,1280])test(`late acceptance snapshot cannot roll a completed conversation back to queued at ${width}px`,async({page,wire})=>{
 let emit!:(event:Event)=>void;wire.stream=new Promise(resolve=>emit=resolve);
 await open(page,width);wire.state='queued';
 let release!:()=>void;wire.snapshotWait=new Promise(resolve=>release=resolve);
 const input=page.getByRole('textbox',{name:'메시지',exact:true});
 try{
  await input.fill('한 번만 실행');await submit(page,'keyboard');
  await expect.poll(()=>wire.snapshotWaiting).toBe(true);
  const run={...wire.snapshot!.runs.at(-1)!,state:'completed' as const,phase:'결과 저장됨',updated_at:Date.now()};
  wire.snapshot!.runs[wire.snapshot!.runs.length-1]=run;
  emit({seq:++wire.snapshot!.last_seq,id:'completed-before-snapshot',kind:'run',data:run,created_at:run.updated_at});
  await expect(page).toHaveURL(/run=focus-turn-1/);
  await expect(page.locator('.conversation-heading .badge')).toHaveText('결과 저장됨');
 }finally{release();}
 await expect.poll(()=>wire.snapshotWaiting).toBe(false);
 // Let the response body and Svelte's reactive update both settle.
 await page.evaluate(()=>new Promise(requestAnimationFrame));await page.evaluate(()=>new Promise(requestAnimationFrame));
 await expect(page.locator('.conversation-heading .badge')).toHaveText('결과 저장됨');
 await input.fill('그대로 이어갈 질문');await expect(page.getByRole('button',{name:'전송',exact:true})).toBeEnabled();
 await expect(page.getByRole('button',{name:'중단',exact:true})).toHaveCount(0);
 expect(wire.submissions).toHaveLength(1);
});

test('newer conversation detail restores completion before the event stream catches up',async({page,wire})=>{
 await open(page,390);wire.state='queued';wire.detailState='completed';
 const input=page.getByRole('textbox',{name:'메시지',exact:true});
 await input.fill('빠르게 끝난 질문');await submit(page,'keyboard');
 await expect(page).toHaveURL(/run=focus-turn-1/);
 await expect(page.locator('.conversation-heading .badge')).toHaveText('결과 저장됨');
 await input.fill('준비한 초안');await expect(input).toHaveValue('준비한 초안');
 await expect(page.getByRole('button',{name:'전송',exact:true})).toBeEnabled();
 await expect(page.getByRole('button',{name:'중단',exact:true})).toHaveCount(0);
});

for(const width of [390,1280])test(`commentary remains inspectable without a second answer bubble at ${width}px`,async({page})=>{
 await page.route('**/api/runs/layout-parent',async route=>{
  const response=await route.fetch(),detail=await response.json();
  detail.messages=[
   {id:'user',run_id:'layout-parent',role:'user',text:'안녕',created_at:1},
   {id:'progress',run_id:'layout-parent',role:'assistant',phase:'commentary',text:'요청을 확인하는 중입니다.',created_at:2},
   {id:'answer',run_id:'layout-parent',role:'assistant',phase:'final_answer',text:'최종 답변입니다.',created_at:3},
   {id:'legacy',run_id:'layout-parent',role:'assistant',text:'단계 정보가 없는 이전 답변',created_at:4}
  ];detail.conversation=detail.messages;
  await route.fulfill({json:detail});
 });
 await open(page,width);
 await expect(page.locator('.message.assistant')).toHaveCount(2);
 await expect(page.getByText('최종 답변입니다.',{exact:true})).toBeVisible();
 await expect(page.getByText('단계 정보가 없는 이전 답변',{exact:true})).toBeVisible();
 const progress=page.locator('.message.commentary');
 await expect(progress.getByText('요청을 확인하는 중입니다.',{exact:true})).toBeHidden();
 await progress.locator('summary').click();
 await expect(progress.getByText('요청을 확인하는 중입니다.',{exact:true})).toBeVisible();
 await progress.locator('summary').click();
 await expect(progress.getByText('요청을 확인하는 중입니다.',{exact:true})).toBeHidden();
});


test('a missed completion event is reconciled without resending the draft',async({page,wire})=>{
 await open(page);wire.state='queued';wire.phase='전송 대기';
 const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('한 번만 전송');await submit(page,'button');
 await expect(page).toHaveURL(/run=focus-turn-1/);await expect(page.locator('.conversation-heading .badge')).toHaveText('전송 대기');
 await input.fill('보존할 후속 질문');wire.detailState='completed';
 await expect(page.locator('.conversation-heading .badge')).toHaveText('결과 저장됨',{timeout:12000});
 await expect(page.getByRole('button',{name:'전송',exact:true})).toBeEnabled();await expect(input).toHaveValue('보존할 후속 질문');expect(wire.submissions).toHaveLength(1);
});

test('/resume opens saved conversations locally without submitting a model turn',async({page,wire})=>{
 await open(page);const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('/resume');await submit(page,'keyboard');
 await expect(page.getByRole('dialog',{name:'세션 검색',exact:true})).toBeVisible();expect(wire.submissions).toHaveLength(0);
 await expect(page.getByRole('searchbox',{name:'세션 검색어'})).toBeVisible();
});


test('an acceptance snapshot missing the new run is repaired by its detail response',async({page,wire})=>{
 await open(page);
 await page.route('**/api/snapshot',route=>route.fulfill({json:{...wire.snapshot!,runs:wire.snapshot!.runs.filter(r=>!r.id.startsWith('focus-turn-'))}}));
 const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('목록 응답이 늦어도 표시');await submit(page,'keyboard');
 await expect(page).toHaveURL(/run=focus-turn-1/);await expect(page.locator('.conversation-heading .badge')).toHaveText('결과 저장됨');
 await expect(input).toBeEditable();await expect(input).toHaveValue('');expect(wire.submissions).toHaveLength(1);
});
