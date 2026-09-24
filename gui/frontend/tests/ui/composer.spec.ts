import {test as base,expect,type Page} from '@playwright/test';
import type {Snapshot,Submission,RunState} from '../../src/lib/types';

type Wire={snapshot?:Snapshot;submissions:Submission[];wait:Promise<void>|null;status:number;state:RunState};
const test=base.extend<{wire:Wire}>({
 wire:[async({page,baseURL},use)=>{
  const errors:string[]=[],wire:Wire={submissions:[],wait:null,status:200,state:'completed'};
  page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/*',async route=>{
   const request=route.request(),url=new URL(request.url());
   if(url.origin===new URL(baseURL!).origin){
    if(request.method()==='GET'){
     if(url.pathname==='/api/snapshot'){
      wire.snapshot??=await (await route.fetch()).json() as Snapshot;
      return route.fulfill({json:wire.snapshot});
     }
     if(url.pathname.startsWith('/api/runs/focus-turn-')){
      const run=wire.snapshot!.runs.find(r=>r.id===url.pathname.split('/').at(-1));
      return route.fulfill({json:{run,messages:[],conversation:[],inbox:[],approvals:[],inputs:[]}});
     }
     return route.continue();
    }
    if(request.method()==='POST'&&url.pathname==='/api/command'){
     const body=request.postDataJSON();
     if(body.type==='submit'){
      const submission=body.request as Submission;
      expect(submission.provider).toBe('mock');
      wire.submissions.push(submission);await wire.wait;
      if(wire.status!==200)return route.fulfill({status:wire.status,json:{error:'검증용 전송 오류'}});
      const snapshot=wire.snapshot!,source=snapshot.runs.find(r=>r.id===submission.target_run_id)??snapshot.runs[0];
      const id='focus-turn-'+wire.submissions.length,workId=submission.work_id??'focus-work';
      const run={...source,id,session_id:submission.mode==='fresh'?id:source.session_id,continued_from:submission.mode==='fresh'?null:source.id,work_id:workId,turn_id:id+'-turn',state:wire.state};
      if(!snapshot.works.some(w=>w.id===workId))snapshot.works.push({...snapshot.works[0],id:workId});
      snapshot.runs.push(run);
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
