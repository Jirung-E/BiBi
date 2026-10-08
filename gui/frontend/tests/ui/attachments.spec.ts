import {test as base,expect,type Page} from '@playwright/test';
import {readFile} from 'node:fs/promises';
import type {Attachment,Detail,Provider,Snapshot,Submission} from '../../src/lib/types';
const png='iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6h0AAAAASUVORK5CYII=';
const picture={name:'스크린샷.png',mimeType:'image/png',buffer:Buffer.from(png,'base64')};
const note={name:'설계.md',mimeType:'text/markdown',buffer:Buffer.from('첨부된 설계 내용')};
type Wire={documents:string[];snapshot?:Snapshot;detail?:Detail;files:Map<string,{attachment:Attachment;data_base64:string}>;uploads:unknown[];submissions:Submission[];uploadStatus:number;submitStatus:number;hold:Promise<void>|null;uploadHold:Promise<void>|null;enabled:boolean|undefined;provider:Provider|null};
const test=base.extend<{wire:Wire}>({wire:[async({page,baseURL},use,testInfo)=>{
 const errors:string[]=[],wire:Wire={documents:[],files:new Map(),uploads:[],submissions:[],uploadStatus:200,submitStatus:200,hold:null,uploadHold:null,enabled:true,provider:null};
 if(testInfo.project.metadata.platform==='Win32')await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{value:'Win32'}));
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());
  if(url.origin===new URL(baseURL!).origin){
   if(req.isNavigationRequest()&&req.resourceType()==='document')wire.documents.push(url.href);
   if(url.pathname==='/api/snapshot'){
    if(!wire.snapshot){
     wire.snapshot=await(await route.fetch()).json() as Snapshot;
     if(wire.provider){wire.snapshot.providers[0].adapter=wire.provider;wire.snapshot.runs[0].provider=wire.provider;}
    }
    return route.fulfill({json:{...wire.snapshot,attachments_v1:wire.enabled}});
   }
   if(url.pathname==='/api/attachments'&&req.method()==='POST'){
    const u=req.postDataJSON();wire.uploads.push(u);await wire.uploadHold;
    if(wire.uploadStatus!==200)return route.fulfill({status:wire.uploadStatus,json:{error:'업로드 시험 오류'}});
    const a:Attachment={id:u.id,project_key:u.project_key,name:u.name,size:Buffer.from(u.data_base64,'base64').length,kind:u.name.endsWith('.png')?'image':u.name.endsWith('.zip')?'file':'text',media_type:u.name.endsWith('.png')?'image/png':'text/plain',sha256:'fixture'};
    wire.files.set(a.id,{attachment:a,data_base64:u.data_base64});return route.fulfill({json:a});
   }
   if(url.pathname.startsWith('/api/attachments/'))return route.fulfill({json:wire.files.get(url.pathname.split('/').at(-1)!)});
   if(url.pathname.startsWith('/api/runs/')){
    if(!wire.detail){
     wire.detail=await(await route.fetch()).json() as Detail;
     if(wire.provider)wire.detail.run.provider=wire.provider;
    }
    return route.fulfill({json:url.pathname.endsWith('/status')?wire.detail.run:wire.detail});
   }
   if(req.method()==='POST'&&url.pathname==='/api/command'){
    const body=req.postDataJSON();
    if(body.type==='select_model')return route.fulfill({json:{}});
    if(body.type==='submit'){
     const r:Submission=body.request;wire.submissions.push(r);await wire.hold;
     if(wire.submitStatus!==200)return route.fulfill({status:wire.submitStatus,json:{error:'전송 시험 오류'}});
     const old=wire.detail!.run,next={...old,id:'attachment-turn-'+wire.submissions.length,continued_from:old.id,turn_id:'attachment-turn-'+wire.submissions.length};
     const message={id:'attached-message-'+wire.submissions.length,run_id:next.id,role:'user',text:r.question,attachments:r.attachments?.map(id=>wire.files.get(id)!.attachment),created_at:Date.now()};
     wire.detail={...wire.detail!,run:next,messages:[message],conversation:[...wire.detail!.conversation,message]};wire.snapshot!.runs.push(next);
     return route.fulfill({json:{submission_id:r.submission_id,run_id:next.id,request_id:next.request_id,work_id:next.work_id,status:'accepted'}});
    }
   }
   if(req.method()==='GET')return route.continue();
  }
  errors.push(req.method()+' '+req.url());return route.abort();
 });
 await use(wire);expect(errors).toEqual([]);
},{auto:true}]});
async function open(page:Page,width=1280){await page.setViewportSize({width,height:844});await page.goto('/?view=conversation&project=layout-project&run=layout-parent');await expect(page.getByRole('textbox',{name:'메시지',exact:true})).toBeVisible();}
const picker=(page:Page)=>page.getByLabel('첨부 파일 선택',{exact:true});
async function ready(page:Page){await expect(page.getByRole('button',{name:'전송',exact:true})).toBeEnabled();}

for(const width of [390,1280])test(`files-only message, preview, download, removal and focus at ${width}px`,async({page,wire})=>{
 await open(page,width);await picker(page).setInputFiles([picture,note]);await ready(page);
 expect(wire.uploads).toHaveLength(2);await expect(page.getByRole('button',{name:'설계.md 첨부 제거'})).toBeVisible();
 await page.getByRole('button',{name:'스크린샷.png 미리보기'}).click();const dialog=page.getByRole('dialog',{name:'스크린샷.png 미리보기'});await expect(dialog.getByRole('img')).toBeVisible();
 await dialog.getByRole('button',{name:'첨부 미리보기 닫기'}).click();
 const download=page.waitForEvent('download');await page.getByRole('button',{name:'설계.md 다운로드'}).click();const saved=await download;
 // WebKit on macOS returns decomposed Hangul; compare the same Unicode name.
 expect(saved.suggestedFilename().normalize('NFC')).toBe('설계.md');expect(await readFile((await saved.path())!)).toEqual(note.buffer);
 await page.getByRole('button',{name:'설계.md 첨부 제거'}).click();await page.getByRole('button',{name:'전송',exact:true}).click();
 await expect(page).toHaveURL(/run=attachment-turn-1/);expect(wire.submissions[0].question).toBe('');expect(wire.submissions[0].attachments).toHaveLength(1);
 await expect(page.locator('.message-attachments').getByRole('button',{name:'스크린샷.png 미리보기'})).toBeVisible();
 await expect(page.locator('.message.user').last().locator('.message-text')).toHaveCount(0);
 const input=page.getByRole('textbox',{name:'메시지',exact:true});await expect(input).toBeFocused();await input.fill('이어서 물어봄');await page.keyboard.press('Control+Enter');await expect(page).toHaveURL(/run=attachment-turn-2/);
 expect(wire.submissions[1].attachments).toBeUndefined();expect(wire.submissions[1].target_run_id).toBe('attachment-turn-1');
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
});

test('paste image and drop file preserve text, and uploaded drafts survive reload',async({page,wire})=>{
 await open(page);const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('기존 초안');
 await input.evaluate((node,png)=>{const dt=new DataTransfer();dt.items.add(new File([Uint8Array.from(atob(png),c=>c.charCodeAt(0))],'붙여넣기.png',{type:'image/png'}));node.dispatchEvent(new ClipboardEvent('paste',{bubbles:true,cancelable:true,clipboardData:dt}));},png);
 await expect(page.getByRole('button',{name:'붙여넣기.png 첨부 제거'})).toBeVisible();
 await input.evaluate(node=>{const dt=new DataTransfer();dt.items.add(new File(['dropped data'],'dropped.txt',{type:'text/plain'}));node.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:dt}));});
 await expect(page.getByRole('button',{name:'dropped.txt 첨부 제거'})).toBeVisible();await expect(input).toHaveValue('기존 초안');
 await page.reload();await expect(page.getByRole('button',{name:'붙여넣기.png 첨부 제거'})).toBeVisible();await expect(input).toHaveValue('기존 초안');
 expect(wire.uploads).toHaveLength(2);await page.getByRole('button',{name:'전송',exact:true}).click();await expect.poll(()=>wire.submissions.length).toBe(1);expect(wire.submissions[0].attachments).toHaveLength(2);
});

test('upload failure retries the same ID and keeps send disabled until all files are ready',async({page,wire})=>{
 await open(page);wire.uploadStatus=503;await picker(page).setInputFiles(note);await expect(page.getByRole('alert')).toHaveText('업로드 시험 오류');await expect(page.getByRole('button',{name:'전송',exact:true})).toBeDisabled();
 wire.uploadStatus=200;await page.getByRole('button',{name:'재시도',exact:true}).click();await ready(page);
 expect((wire.uploads[0] as {id:string}).id).toBe((wire.uploads[1] as {id:string}).id);
 wire.submitStatus=409;await page.getByRole('button',{name:'전송',exact:true}).click();await expect(page.getByRole('alert')).toHaveText('전송 시험 오류');await expect(page.getByRole('button',{name:'설계.md 첨부 제거'})).toBeVisible();
 wire.submitStatus=200;await page.getByRole('button',{name:'전송',exact:true}).click();await expect(page).toHaveURL(/attachment-turn-2/);expect(wire.uploads).toHaveLength(2);
});

test('attachment sent with ambiguous acceptance reuses submission ID without another upload',async({page,wire})=>{
 await open(page);await picker(page).setInputFiles(note);await ready(page);wire.submitStatus=503;
 await page.getByRole('button',{name:'전송',exact:true}).click();await expect(page.getByRole('alert')).toContainText('같은 전송을 재시도');
 await page.route('**/api/submissions/*',route=>route.fulfill({status:404,json:{error:'not found'}}));wire.submitStatus=200;
 await page.getByRole('button',{name:'접수 확인·재시도'}).click();await expect(page).toHaveURL(/attachment-turn-2/);expect(wire.submissions[1]).toEqual(wire.submissions[0]);expect(wire.uploads).toHaveLength(1);
});

test('unsupported binary and slash command keep attachments instead of silently dropping them',async({page,wire})=>{
 // Configure the first responses; a setup-only reload can abort WebKit's open EventSource.
 wire.provider='ollama';await open(page);
 await picker(page).setInputFiles({name:'data.zip',mimeType:'application/zip',buffer:Buffer.from([0,1,2])});await expect(page.getByRole('alert')).toContainText('Ollama에는 이미지·텍스트');await expect(page.getByRole('button',{name:'전송',exact:true})).toBeDisabled();
 await page.getByRole('button',{name:'data.zip 첨부 제거'}).click();await picker(page).setInputFiles(note);await ready(page);await page.getByRole('textbox',{name:'메시지',exact:true}).fill('/resume');
 await page.getByRole('button',{name:'전송',exact:true}).click();await expect(page.getByRole('alert')).toHaveText('슬래시 명령과 첨부 파일은 따로 보내세요.');expect(wire.submissions).toHaveLength(0);await expect(page.getByRole('button',{name:'설계.md 첨부 제거'})).toBeVisible();
 expect(wire.documents).toHaveLength(1);
});

for(const enabled of [false,undefined])test(`old server with attachments capability ${enabled===undefined?'absent':'disabled'} explains the limit without sending files`,async({page,wire})=>{
 wire.enabled=enabled;await open(page);await expect(page.getByRole('button',{name:'파일 첨부',exact:true})).toBeDisabled();await expect(page.getByRole('button',{name:'파일 첨부',exact:true})).toHaveAttribute('title',/업데이트/);
 expect(wire.uploads).toHaveLength(0);expect(wire.submissions).toHaveLength(0);expect(wire.documents).toHaveLength(1);
});

test('excessive file selection gives explicit limits before uploading',async({page,wire})=>{
 await open(page);await expect(page.getByRole('button',{name:'파일 첨부',exact:true})).toBeEnabled();await picker(page).setInputFiles(Array.from({length:9},(_,i)=>({...note,name:`${i}.txt`})));
 await expect(page.getByRole('alert')).toContainText('최대 8개');expect(wire.uploads).toHaveLength(0);
 await picker(page).setInputFiles({...note,buffer:Buffer.alloc(8*1024*1024+1)});await expect(page.getByRole('alert')).toContainText('파일당 최대 8 MiB');expect(wire.uploads).toHaveLength(0);
 expect(wire.submissions).toHaveLength(0);expect(wire.documents).toHaveLength(1);
});

test('an upload finishing after navigation stays with its original draft',async({page,wire})=>{
 await open(page);let release!:()=>void;wire.uploadHold=new Promise<void>(r=>release=r);
 await picker(page).setInputFiles(note);await expect.poll(()=>wire.uploads.length).toBe(1);await expect(page.getByRole('button',{name:'전송',exact:true})).toBeDisabled();
 try{await page.getByRole('button',{name:'사용량·연결',exact:true}).click();}finally{release();}
 await expect.poll(()=>page.evaluate(()=>Object.keys(localStorage).filter(k=>k.startsWith('bibi:draft:')).some(k=>(JSON.parse(localStorage[k]).attachments??[]).length===1))).toBe(true);
 await page.getByRole('button',{name:'작업 대화',exact:true}).click();await expect(page.getByRole('button',{name:'설계.md 첨부 제거'})).toBeVisible();
});
