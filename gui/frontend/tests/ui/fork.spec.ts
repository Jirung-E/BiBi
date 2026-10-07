import {test,expect} from '@playwright/test';
import type {Snapshot,Run} from '../../src/lib/types';
for(const width of [390,1280])test('fork selected point stays independent at '+width,async({page,baseURL},info)=>{
 await page.setViewportSize({width,height:900});await page.clock.install();
 await page.addInitScript(()=>Object.defineProperty(crypto,'randomUUID',{value:undefined}));
 if(info.project.metadata.platform)await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{value:'Win32'}));
 let snapshot:Snapshot;let created:Run|undefined;const commands:any[]=[];const errors:string[]=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());
  if(url.origin!==new URL(baseURL!).origin){errors.push('external request');return route.abort();}
  if(url.pathname==='/api/snapshot'){
   snapshot??=await(await route.fetch()).json();snapshot.session_fork_v1=true;
   if(created&&!snapshot.runs.some(r=>r.id===created!.id))snapshot.runs.push(created);
   return route.fulfill({json:snapshot});
  }
  if(req.method()==='POST'){
   const body=req.postDataJSON();commands.push(body);
   if(body.type==='list_fork_points')return route.fulfill({json:{workspace:'/fixture/shared',points:[{id:'first',revision:'frozen-first',excerpt:'첫 번째 답변',created_at:1},{id:'later',revision:'frozen-later',excerpt:'이후 답변',created_at:2}]}});
   if(body.type==='fork_session'){
    const original=snapshot.runs.find(r=>r.id==='layout-parent')!;
    created={...structuredClone(original),id:'fork-run',session_id:'fork-session',session_key:'fork-native',title:body.request.title,parent_session_id:null,parent_run_id:null,runtime:{...original.runtime,fork:{session_id:original.session_id,run_id:original.id,point_id:body.request.point_id}}};
    return route.fulfill({json:created});
   }
   if(body.type==='submit')return route.fulfill({status:409,json:{error:'후속 전송 확인'}});
   errors.push(body.type);return route.abort();
  }
  if(url.pathname==='/api/runs/fork-run')return route.fulfill({json:{run:created,conversation:[{id:'copied',run_id:'fork-run',role:'assistant',text:'첫 번째 답변',created_at:1}],messages:[],approvals:[],inbox:[],inputs:[]}});
  if(url.pathname==='/api/runs/fork-run/status')return route.fulfill({json:created});
  return route.continue();
 });
 await page.goto('/?view=conversation&project=layout-project&run=layout-parent');
 await page.getByRole('button',{name:'세션 포크',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'세션 포크',exact:true});
 await expect(dialog.getByLabel('분기 지점')).toHaveValue('later');
 await dialog.getByLabel('분기 지점').selectOption('first');
 await dialog.getByLabel('이름',{exact:true}).fill('다른 접근');
 await expect(dialog.getByLabel('작업 폴더')).toHaveValue('/fixture/shared');
 await expect(dialog).toContainText('작업 폴더와 파일은 원본과 공유합니다.');
 const box=await dialog.boundingBox();expect(box!.x).toBeGreaterThanOrEqual(0);expect(box!.x+box!.width).toBeLessThanOrEqual(width+1);
 await dialog.getByRole('button',{name:'여기서 포크'}).click();
 await expect(page).toHaveURL(/run=fork-run/);await expect(dialog).toHaveCount(0);
 const heading=page.locator('.conversation-heading>div:first-child strong');await expect(heading).toHaveText('다른 접근');expect(await heading.evaluate(el=>el.scrollWidth<=el.clientWidth)).toBe(true);
 await expect(page.locator('.messages')).toContainText('첫 번째 답변');await expect(page.locator('.messages')).not.toContainText('이후 답변');
 const request=commands.find(c=>c.type==='fork_session').request;
 expect(request.point_id).toBe('first');expect(request.revision).toBe('frozen-first');expect(request.run_id).toBe('layout-parent');expect(request.request_id).toMatch(/^submission_/);
 const input=page.getByRole('textbox',{name:'메시지',exact:true});await input.fill('포크 후속');await input.press('Control+Enter');
 await expect(page.getByText('후속 전송 확인',{exact:true})).toBeVisible();
 expect(commands.find(c=>c.type==='submit').request).toMatchObject({mode:'continue',target_run_id:'fork-run'});
 await page.getByRole('button',{name:'원본 대화',exact:true}).click();await expect(page).toHaveURL(/run=layout-parent/);
 await page.goto('/?view=canvas&project=layout-project');
 await page.getByRole('button',{name:'전체 보기',exact:true}).click();
 await expect(page.locator('.fork-edge')).toHaveCount(1);
 await expect(page.locator('[data-session-id="fork-session"]')).toContainText('포크');
 expect(errors).toEqual([]);
});
test('fork load failure is visible and retryable',async({page})=>{
 let calls=0;
 await page.route('**/api/snapshot',async route=>{const data=await(await route.fetch()).json();data.session_fork_v1=true;await route.fulfill({json:data});});
 await page.route('**/api/command',async route=>{
  const body=route.request().postDataJSON();expect(body.type).toBe('list_fork_points');calls++;
  return calls===1?route.fulfill({status:409,json:{error:'원본 파일을 확인하세요'}}):route.fulfill({json:{workspace:'/fixture',points:[]}});
 });
 await page.goto('/?view=conversation&project=layout-project&run=layout-parent');await page.getByRole('button',{name:'세션 포크',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'세션 포크',exact:true});
 await expect(dialog.getByRole('alert')).toHaveText('원본 파일을 확인하세요');await dialog.getByRole('button',{name:'분기 지점 다시 확인'}).click();
 await expect(dialog).toContainText('포크할 수 있는 완료된 답변이 없습니다.');expect(calls).toBe(2);
});
