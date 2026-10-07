import {test,expect} from '@playwright/test';
import type {Snapshot,Run} from '../../src/lib/types';
for(const origin of ['managed','external'] as const)test('/resume selection preserves intent for '+origin,async({page,baseURL},info)=>{
 // This checks request routing, independently of headless display-frame scheduling.
 // Keep the clock ticking; native animation timing has separate canvas/layout tests.
 await page.clock.install();
 if(info.project.metadata.platform)await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{value:'Win32'}));
 let snapshot:Snapshot;const submissions:any[]=[],errors:string[]=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const request=route.request(),url=new URL(request.url());
  if(url.origin!==new URL(baseURL!).origin){errors.push('external request');return route.abort();}
  if(request.method()==='POST'){
   const body=request.postDataJSON();
   if(body.type==='submit'){submissions.push(body.request);return route.fulfill({status:409,json:{error:'모의 전송 확인'}});}
   errors.push('unexpected '+body.type);return route.abort();
  }
  if(url.pathname==='/api/snapshot'){
   snapshot??=await(await route.fetch()).json();const r=snapshot.runs.find(r=>r.id==='layout-parent')!;
   r.origin=origin;r.state=origin==='external'?'uncertain':'completed';r.phase=origin==='external'?'외부 Claude 실행 상태 확인 필요':'결과 저장됨';
   r.capabilities.continue_session={supported:origin==='managed',reason:origin==='external'?'입력 채널 없음':null};
   return route.fulfill({json:snapshot});
  }
  if(url.pathname==='/api/sessions/search')return route.fulfill({json:[{run:snapshot.runs.find(r=>r.id==='layout-parent'),excerpt:'resume fixture'}]});
  if(url.pathname.startsWith('/api/runs/')){
   const data=await(await route.fetch()).json(),run=snapshot.runs.find(r=>r.id===url.pathname.split('/')[3]);
   return route.fulfill({json:url.pathname.endsWith('/status')?run:{...data,run}});
  }
  return route.continue();
 });
 await page.goto('/?view=conversation&project=layout-project&run=layout-other');
 const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('/resume');await input.press('Control+Enter');
 const dialog=page.getByRole('dialog',{name:'세션 검색',exact:true});
 await expect(dialog.locator('.search-result')).toContainText(origin==='managed'?'이어가기':'기록 보기');await dialog.locator('.search-result').click();
 await expect(dialog).toHaveCount(0);await expect(page).toHaveURL(/run=layout-parent/);
 await input.fill('이어갈 질문');
 const send=page.getByRole('button',{name:'전송',exact:true});
 if(origin==='external'){
  await expect(send).toBeDisabled();await input.press('Control+Enter');expect(submissions).toHaveLength(0);
  await expect(page.getByRole('checkbox')).toHaveCount(0);
  await expect(page.getByText('외부 세션 · 원래 앱에서 이어갈 수 있습니다.',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'새 세션으로 시작',exact:true}).click();
 }
 await send.click();await expect(page.getByText('모의 전송 확인',{exact:true})).toBeVisible();
 expect(submissions).toHaveLength(1);expect(submissions[0].mode).toBe(origin==='external'?'fresh':'continue');expect(submissions[0].target_run_id).toBe('layout-parent');
 expect(errors).toEqual([]);
});
