import {test as base,expect,type Page} from '@playwright/test';
import type {Snapshot,Detail,Submission} from '../../src/lib/types';
type Wire={snapshot?:Snapshot;detail?:Detail;wait:Promise<void>|null;waiting:number;failure:boolean;table:string|null;claude:boolean;submissions:Submission[]};
const test=base.extend<{wire:Wire}>({wire:[async({page,baseURL},use)=>{
 const errors:string[]=[],wire:Wire={wait:null,waiting:0,failure:false,table:null,claude:false,submissions:[]};
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());
  if(url.origin!==new URL(baseURL!).origin){errors.push('external request');return route.abort();}
  if(req.method()==='GET'){
   if(url.pathname==='/api/snapshot'){
    if(!wire.snapshot){wire.snapshot=await(await route.fetch()).json();if(wire.claude){for(const p of wire.snapshot!.providers)p.adapter='claude';for(const r of wire.snapshot!.runs){r.provider='claude';r.approval_mode='accept_edits';}}}
    return route.fulfill({json:wire.snapshot});
   }
   if(url.pathname.startsWith('/api/runs/')){
    const id=url.pathname.split('/')[3],run=wire.snapshot!.runs.find(r=>r.id===id)!;
    if(url.pathname.endsWith('/status'))return route.fulfill({json:run});
    if(id.startsWith('chat-next-')){
     wire.waiting++;await wire.wait;
     if(wire.failure)return route.fulfill({status:503,json:{error:'검증용 이력 조회 실패'}});
     const messages=[...(wire.detail!.conversation??wire.detail!.messages),{...wire.detail!.messages[0],id:'new-question',role:'user',text:wire.submissions.at(-1)!.question}];
     return route.fulfill({json:{...wire.detail,run,messages,conversation:messages,approvals:[],inputs:[]}});
    }
    const detail=await(await route.fetch()).json() as Detail;detail.run=run;
    if(wire.table){detail.messages=[{...detail.messages[0],id:'tables',role:'assistant',text:wire.table}];detail.conversation=detail.messages;}
    if(wire.claude){detail.approvals=[{id:'edit-request',run_id:run.id,native_id:'permission',kind:'claude/requestApproval',title:'Claude · Edit',state:'pending',created_at:0,detail:{tool_name:'Edit',input:{file_path:'/project/src/main.rs',old_string:'old',new_string:'new'},description:'코드 수정',decision_reason:'Explicit ask rule requires approval'}}];}
    if(id==='layout-parent')wire.detail=detail;
    return route.fulfill({json:detail});
   }
   return route.continue();
  }
  if(req.method()==='POST'&&url.pathname==='/api/command'){
   const body=req.postDataJSON();if(body.type==='select_model')return route.fulfill({json:{}});
   if(body.type==='submit'){
    const s=body.request as Submission;wire.submissions.push(s);const prior=wire.snapshot!.runs.find(r=>r.id===s.target_run_id)!;
    const id='chat-next-'+wire.submissions.length;wire.snapshot!.runs.push({...prior,id,continued_from:prior.id,state:'completed',updated_at:prior.updated_at+10});wire.snapshot!.last_seq++;
    return route.fulfill({json:{submission_id:s.submission_id,run_id:id,request_id:id,work_id:prior.work_id,status:'accepted'}});
   }
  }
  errors.push(req.method()+' '+url.pathname);return route.abort();
 });
 await use(wire);expect(errors).toEqual([]);
},{auto:true}]});
test.beforeEach(async({page},info)=>{const platform=info.project.metadata.platform as string|undefined;if(platform)await page.addInitScript(v=>Object.defineProperty(navigator,'platform',{value:v}),platform);});
async function open(page:Page,width=1280,height=844){await page.setViewportSize({width,height});await page.goto('/?view=conversation&project=layout-project&run=layout-parent');await expect(page.locator('.messages .message').first()).toBeAttached();}
for(const failure of [false,true])test(`sending keeps existing chat DOM while detail ${failure?'fails':'loads'}`,async({page,wire})=>{
 await open(page);const old=await page.locator('.messages .message').first().elementHandle();
 let release!:()=>void;wire.wait=new Promise<void>(r=>release=r);wire.failure=failure;
 try{
  await page.getByRole('textbox',{name:'메시지',exact:true}).fill('같은 대화의 다음 질문');await page.getByRole('button',{name:'전송',exact:true}).click();
  await expect(page).toHaveURL(/run=chat-next-1/);await expect.poll(()=>wire.waiting).toBeGreaterThan(0);
  expect(await old!.evaluate(e=>e.isConnected)).toBe(true);await expect(page.getByText('기록 불러오는 중…',{exact:true})).toHaveCount(0);
  release();wire.wait=null;
  if(failure)await expect(page.getByText('검증용 이력 조회 실패',{exact:true})).toBeVisible();else await expect(page.locator('.message-text').filter({hasText:'같은 대화의 다음 질문'})).toBeVisible();
  expect(await old!.evaluate(e=>e.isConnected)).toBe(true);
  await page.getByRole('button',{name:'세션 캔버스',exact:true}).click();await expect(page.locator('.messages .message')).toHaveCount(0);
 }finally{release();}
});
test('composer top handle resizes upward, keeps draft and restores its size',async({page},info)=>{
 await open(page);const input=page.getByRole('textbox',{name:'메시지',exact:true}),grip=page.getByRole('separator',{name:'입력창 높이',exact:true});
 await input.fill('크기 조절 중에도 유지할 초안');const before=(await input.boundingBox())!,box=(await grip.boundingBox())!;
 expect(box.y+box.height).toBeLessThanOrEqual(before.y+1);
 await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();await page.mouse.move(box.x+box.width/2,box.y-110,{steps:8});await page.mouse.up();
 await expect.poll(async()=>(await input.boundingBox())!.height).toBeGreaterThan(before.height+60);
 const height=(await input.boundingBox())!.height;await expect(input).toHaveValue('크기 조절 중에도 유지할 초안');await page.screenshot({path:info.outputPath('resized-composer.png')});
 await page.reload();await expect(input).toHaveValue('크기 조절 중에도 유지할 초안');await expect.poll(async()=>Math.abs((await input.boundingBox())!.height-height)).toBeLessThan(2);
 await grip.focus();await page.keyboard.press('ArrowDown');await expect.poll(async()=>(await input.boundingBox())!.height).toBeLessThan(height-5);
 await grip.dblclick();await expect.poll(async()=>Math.abs((await input.boundingBox())!.height-before.height)).toBeLessThan(2);
});
for(const theme of ['light','dark'] as const)test(`composer resize clamps to enlarged mobile viewport in ${theme}`,async({page},info)=>{
 await page.emulateMedia({colorScheme:theme});await page.addInitScript(()=>{localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:2}));localStorage.setItem('bibi:composerHeight','30');});
 await open(page,390,650);const input=page.getByRole('textbox',{name:'메시지',exact:true}),grip=page.getByRole('separator',{name:'입력창 높이',exact:true});
 await grip.focus();await page.keyboard.press('End');await input.fill('모바일 초안');
 const send=page.getByRole('button',{name:'전송',exact:true});await send.scrollIntoViewIfNeeded();await expect(send).toBeInViewport();
 expect(await page.evaluate(()=>document.documentElement.scrollHeight<=innerHeight+1&&document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
 await page.setViewportSize({width:390,height:450});await send.scrollIntoViewIfNeeded();await expect(send).toBeInViewport();await expect(input).toHaveValue('모바일 초안');await page.screenshot({path:info.outputPath('enlarged-mobile.png')});
});
test('each rendered table copies spreadsheet text and sanitized HTML on HTTP fallback',async({page,wire},info)=>{
 wire.table='| 항목 | 값 |\n|---|---|\n| **하나** | 1 |\n| 둘 | A<br>B |\n\n| 별도 | 표 |\n|---|---|\n| 끝 | 2 |';
 await page.addInitScript(()=>{Object.defineProperty(navigator,'clipboard',{value:undefined,configurable:true});Object.defineProperty(document,'execCommand',{value:(cmd:string)=>{if(cmd!=='copy')return false;const event=new Event('copy',{cancelable:true});const data:Record<string,string>={};Object.defineProperty(event,'clipboardData',{value:{setData:(k:string,v:string)=>data[k]=v}});document.dispatchEvent(event);(window as unknown as {copied:unknown}).copied=data;return event.defaultPrevented;},configurable:true});});
 await open(page,390);const buttons=page.getByRole('button',{name:'표 복사',exact:true});await expect(buttons).toHaveCount(2);await page.screenshot({path:info.outputPath('table-tools.png')});await buttons.first().click();
 const copied=await page.evaluate(()=>(window as unknown as {copied:Record<string,string>}).copied);
 expect(copied['text/plain']).toBe('항목\t값\n하나\t1\n둘\t"A\nB"');expect(copied['text/html']).toContain('<table>');expect(copied['text/html']).not.toContain('button');await expect(page.getByText('복사됨',{exact:true})).toBeVisible();
 await buttons.last().click();expect(await page.evaluate(()=>(window as unknown as {copied:Record<string,string>}).copied['text/plain'])).toBe('별도\t표\n끝\t2');
 await page.getByRole('button',{name:'마크다운 복사',exact:true}).first().click();expect(await page.evaluate(()=>(window as unknown as {copied:Record<string,string>}).copied)).toEqual({'text/plain':'| 항목 | 값 |\n| --- | --- |\n| **하나** | 1 |\n| 둘 | A<br>B |'});
 await page.getByRole('button',{name:'마크다운 복사',exact:true}).last().click();expect(await page.evaluate(()=>(window as unknown as {copied:Record<string,string>}).copied['text/plain'])).toBe('| 별도 | 표 |\n| --- | --- |\n| 끝 | 2 |');
});
test('table copy failure is shown and retriable',async({page,wire})=>{
 wire.table='| a | b |\n|---|---|\n| 1 | 2 |';await page.addInitScript(()=>{Object.defineProperty(navigator,'clipboard',{value:undefined,configurable:true});Object.defineProperty(document,'execCommand',{value:()=>false,configurable:true});});
 await open(page);await page.getByRole('button',{name:'표 복사',exact:true}).click();await expect(page.getByRole('alert').filter({hasText:'복사'})).toBeVisible();await expect(page.getByRole('button',{name:'표 복사',exact:true})).toBeEnabled();
});
test('Claude approval exposes selected mode, file and provider reason',async({page,wire})=>{
 wire.claude=true;await open(page);const card=page.locator('.approval');await card.scrollIntoViewIfNeeded();await expect(card.getByText('편집 자동 승인',{exact:true})).toBeVisible();await expect(card.getByText('/project/src/main.rs',{exact:true})).toBeVisible();await expect(card.getByText('Explicit ask rule requires approval',{exact:true})).toBeVisible();
});

test('late detail and pending approvals never leak into another selected session',async({page,wire})=>{
 wire.claude=true;await open(page);await expect(page.locator('.approval')).toHaveCount(1);
 let release!:()=>void;wire.wait=new Promise<void>(r=>release=r);
 try{
  await page.getByRole('textbox',{name:'메시지',exact:true}).fill('다음 실행');await page.getByRole('button',{name:'전송',exact:true}).click();
  await expect(page).toHaveURL(/run=chat-next-1/);await expect.poll(()=>wire.waiting).toBeGreaterThan(0);await expect(page.locator('.approval')).toHaveCount(0);
  await page.getByRole('button',{name:'세션 캔버스',exact:true}).click();release();wire.wait=null;
  await expect(page.locator('.messages')).toHaveCount(0);await expect(page).not.toHaveURL(/view=conversation/);
 }finally{release();}
});
test('touch-sized top handle preserves the draft, cancels a drag and clamps on viewport changes',async({page})=>{
 await open(page,390,844);const input=page.getByRole('textbox',{name:'메시지',exact:true}),grip=page.getByRole('separator',{name:'입력창 높이',exact:true});
 await input.fill('터치 초안');const before=(await input.boundingBox())!.height;
 await grip.dispatchEvent('pointerdown',{pointerId:4,pointerType:'touch',button:0,clientY:600});await grip.dispatchEvent('pointermove',{pointerId:4,pointerType:'touch',clientY:500});await grip.dispatchEvent('pointerup',{pointerId:4,pointerType:'touch',clientY:500});
 await expect.poll(async()=>(await input.boundingBox())!.height).toBeGreaterThan(before+50);const height=(await input.boundingBox())!.height,pref=await page.evaluate(()=>localStorage.getItem('bibi:composerHeight'));
 const box=(await grip.boundingBox())!;await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();await page.mouse.move(box.x+box.width/2,box.y-60,{steps:3});await page.keyboard.press('Escape');await page.mouse.up();
 await expect.poll(async()=>Math.abs((await input.boundingBox())!.height-height)).toBeLessThan(2);expect(await page.evaluate(()=>localStorage.getItem('bibi:composerHeight'))).toBe(pref);await expect(input).toHaveValue('터치 초안');
 await page.setViewportSize({width:390,height:450});await page.setViewportSize({width:390,height:844});await expect.poll(async()=>Math.abs((await input.boundingBox())!.height-height)).toBeLessThan(2);
});

for(const theme of ['light','dark'] as const)test(`Markdown copy controls fit narrow enlarged tables in ${theme}`,async({page,wire},info)=>{
 wire.table='| a | b |\n|:---|---:|\n| **한글** | 2 |';
 await page.emulateMedia({colorScheme:theme});await page.addInitScript(()=>{localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:2}));Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async(text:string)=>{(window as unknown as {copiedMarkdown:string}).copiedMarkdown=text;}}});});
 await open(page,320,844);const button=page.getByRole('button',{name:'마크다운 복사',exact:true});await button.scrollIntoViewIfNeeded();await expect(button).toBeInViewport();
 const box=(await button.boundingBox())!;expect(box.x).toBeGreaterThanOrEqual(0);expect(box.x+box.width).toBeLessThanOrEqual(321);await button.click();
 expect(await page.evaluate(()=>(window as unknown as {copiedMarkdown:string}).copiedMarkdown)).toBe('| a | b |\n| :--- | ---: |\n| **한글** | 2 |');
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);await page.screenshot({path:info.outputPath('markdown-copy.png')});
});
