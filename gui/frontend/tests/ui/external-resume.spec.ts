import {test,expect,type Page} from '@playwright/test';
import type {Snapshot} from '../../src/lib/types';

async function wire(page:Page,baseURL:string,confirm=true){
 let snapshot:Snapshot;const calls:any[]=[],errors:string[]=[];let fail=confirm;
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());
  if(url.origin!==new URL(baseURL).origin){errors.push('external request');return route.abort();}
  if(req.method()==='POST'){
   const body=req.postDataJSON();calls.push(body);
   if(body.type==='check_external_resume')return route.fulfill({json:{native_id:'native-kept',requires_confirmation:confirm}});
   if(body.type==='resume_external'){
    if(fail){fail=false;return route.fulfill({status:409,json:{error:'원래 세션이 아직 실행 중입니다.'}});}
    const run=snapshot.runs.find(r=>r.id==='layout-parent')!;run.origin='managed';run.state='interrupted';run.phase='이어받기 준비 완료';run.capabilities.continue_session={supported:true,reason:null};
    return route.fulfill({json:run});
   }
   if(body.type==='submit')return route.fulfill({status:409,json:{error:'원래 대화 전송 확인'}});
   errors.push('unexpected '+body.type);return route.abort();
  }
  if(url.pathname==='/api/snapshot'){
   if(!snapshot){snapshot=await(await route.fetch()).json();snapshot.external_resume_v1=true;
    const run=snapshot.runs.find(r=>r.id==='layout-parent')!;run.origin='external';run.provider='claude';run.state=confirm?'uncertain':'disconnected';run.session_key='native-kept';run.capabilities={...run.capabilities,continue_session:{supported:false,reason:'external'}};snapshot.providers[0].adapter='claude';
   }
   return route.fulfill({json:snapshot});
  }
  if(url.pathname==='/api/sessions/search')return route.fulfill({json:[{run:snapshot.runs.find(r=>r.id==='layout-parent'),excerpt:'original conversation'}]});
  if(url.pathname.startsWith('/api/runs/')){
   const data=await(await route.fetch()).json(),run=snapshot.runs.find(r=>r.id===url.pathname.split('/')[3]);
   return route.fulfill({json:url.pathname.endsWith('/status')?run:{...data,run}});
  }
  return route.continue();
 });
 return {calls,errors};
}
test.beforeEach(async({page},info)=>{if(info.project.metadata.platform)await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{value:'Win32'}));});
for(const width of [390,1280])test('takeover preserves draft and original target at '+width,async({page,baseURL})=>{
 await page.setViewportSize({width,height:900});const {calls,errors}=await wire(page,baseURL!);
 await page.goto('/?view=conversation&project=layout-project&run=layout-other');
 const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('/resume');await input.press('Control+Enter');
 const dialog=page.getByRole('dialog',{name:'세션 검색',exact:true});await expect(dialog.locator('.search-result')).toContainText('종료 후 이어받기');await dialog.locator('.search-result').click();
 await expect(page).toHaveURL(/run=layout-parent/);
 const send=page.getByRole('button',{name:'전송',exact:true});
 await input.fill('기존 대화에 이어갈 질문');await expect(send).toBeDisabled();
 await page.getByRole('button',{name:'BiBi에서 이어받기',exact:true}).click();
 const resume=page.getByRole('button',{name:'이어받기',exact:true});await expect(resume).toBeDisabled();
 await page.getByRole('checkbox',{name:'원래 앱에서 이 세션을 종료했습니다'}).check();await resume.click();
 await expect(page.getByRole('alert')).toContainText('아직 실행 중');await expect(send).toBeDisabled();await expect(input).toHaveValue('기존 대화에 이어갈 질문');
 await page.getByRole('button',{name:'BiBi에서 이어받기',exact:true}).click();
 await page.getByRole('checkbox',{name:'원래 앱에서 이 세션을 종료했습니다'}).check();await resume.click();
 await expect(page.getByRole('button',{name:'BiBi에서 이어받기',exact:true})).toHaveCount(0);await expect(send).toBeEnabled();
 await expect(input).toHaveValue('기존 대화에 이어갈 질문');await expect(input).toBeFocused();
 await send.click();await expect(page.getByText('원래 대화 전송 확인',{exact:true})).toBeVisible();
 const submissions=calls.filter(c=>c.type==='submit');expect(submissions).toHaveLength(1);expect(submissions[0].request.mode).toBe('continue');expect(submissions[0].request.target_run_id).toBe('layout-parent');
 expect(calls.filter(c=>c.type==='resume_external').every(c=>c.expected_native_id==='native-kept'&&c.confirmed_stopped===true)).toBe(true);
 expect(errors).toEqual([]);
});
test('confirmed dead session transfers without a model submission',async({page,baseURL})=>{
 const {calls,errors}=await wire(page,baseURL!,false);
 await page.goto('/?view=conversation&project=layout-project&run=layout-parent');
 await page.getByRole('textbox',{name:'메시지',exact:true}).fill('초안');
 await page.getByRole('button',{name:'BiBi에서 이어받기',exact:true}).click();
 await expect(page.getByRole('button',{name:'전송',exact:true})).toBeEnabled();
 expect(calls.map(c=>c.type)).toEqual(['check_external_resume','resume_external']);expect(calls[1].confirmed_stopped).toBe(false);
 await expect(page.getByRole('checkbox')).toHaveCount(0);expect(errors).toEqual([]);
});
