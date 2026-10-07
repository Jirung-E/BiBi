import {test,expect,type Page,type TestInfo} from '@playwright/test';
import type {Snapshot} from '../../src/lib/types';

async function fixture(page:Page,baseURL:string,info:TestInfo){
 if(info.project.metadata.platform)await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{value:'Win32'}));
 const errors:string[]=[],calls:Record<string,any>[]=[];let data:Snapshot;
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());
  if(url.origin!==new URL(baseURL).origin){errors.push('external request');return route.abort();}
  if(req.method()==='GET'){
   if(url.pathname==='/api/snapshot'){data??=await(await route.fetch()).json();return route.fulfill({json:data});}
   if(url.pathname.startsWith('/api/runs/')){
    const original=await(await route.fetch()).json(),run=data.runs.find(r=>r.id===url.pathname.split('/')[3]);
    return route.fulfill({json:url.pathname.endsWith('/status')?run:{...original,run}});
   }
   return route.continue();
  }
  const body=req.postDataJSON();calls.push(body);
  if(body.type==='set_session_groups'){
   const run=data.runs.find(r=>r.id===body.run_id)!;
   data.session_groups=[...(data.session_groups??[]).filter(g=>g.session_id!==run.session_id),{session_id:run.session_id,work_ids:body.work_ids}];data.last_seq++;
   return route.fulfill({json:data.session_groups.at(-1)});
  }
  if(body.type==='submit')return route.fulfill({status:409,json:{error:'모의 전송 확인'}});
  if(body.type==='select_model')return route.fulfill({json:{}});
  errors.push('unexpected '+body.type);return route.abort();
 });
 return {calls,errors,snapshot:()=>data};
}
async function fit(page:Page){await page.getByRole('button',{name:'전체 보기',exact:true}).click();}
const node=(page:Page,group:string)=>page.locator('.run-node[data-session-id="layout-parent"][data-group-id="'+group+'"]');
async function editor(page:Page){await page.getByRole('complementary',{name:'세션 상세',exact:true}).getByRole('button',{name:'그룹 연결',exact:true}).click();return page.getByRole('dialog',{name:'그룹 연결',exact:true});}

for(const width of [1280,390])test('shared session references preserve one chat and independent layout at '+width,async({page,baseURL},info)=>{
 const wire=await fixture(page,baseURL!,info);await page.setViewportSize({width,height:1000});
 await page.goto('/?view=canvas&project=layout-project');await fit(page);await node(page,'layout-work').click();
 const before=JSON.stringify(wire.snapshot().runs),dialog=await editor(page);
 await dialog.getByRole('checkbox',{name:'다른 업무',exact:true}).check();
 const box=await dialog.boundingBox();expect(box!.x).toBeGreaterThanOrEqual(0);expect(box!.x+box!.width).toBeLessThanOrEqual(width+1);
 const dimensions=await dialog.locator('.group-options label').evaluateAll(labels=>labels.map(label=>{const control=label.querySelector('input')!.getBoundingClientRect(),text=label.querySelector('span')!.getBoundingClientRect(),row=label.getBoundingClientRect();return {control:{width:control.width,height:control.height},text:{width:text.width,right:text.right},right:row.right};}));
 for(const item of dimensions){expect(Math.abs(item.control.width-item.control.height)).toBeLessThan(1);expect(item.text.width).toBeGreaterThan(item.control.width);expect(item.text.right).toBeLessThanOrEqual(item.right);}

 await dialog.getByRole('button',{name:'저장',exact:true}).click();await expect(dialog).toHaveCount(0);await fit(page);
 await expect(page.locator('.run-node[data-session-id="layout-parent"]')).toHaveCount(2);
 expect(JSON.stringify(wire.snapshot().runs)).toBe(before);expect(wire.snapshot().transmissions).toHaveLength(1);
 await expect(node(page,'layout-work')).toHaveAttribute('aria-pressed','true');await expect(node(page,'layout-other-work')).toHaveAttribute('aria-pressed','true');
 await expect(node(page,'layout-other-work').getByLabel('공유 세션 · 2개 그룹')).toBeVisible();
 const position=()=>node(page,'layout-work').evaluate(e=>(e as HTMLElement).style.left);
 const original=await position(),other=await node(page,'layout-other-work').evaluate(e=>(e as HTMLElement).style.left);
 const group=page.locator('.work-group[data-work-id="layout-other-work"]');await group.focus();await page.keyboard.press('ArrowRight');
 expect(await position()).toBe(original);
 expect(await node(page,'layout-other-work').evaluate(e=>(e as HTMLElement).style.left)).not.toBe(other);
 await expect(page.locator('.connections > g')).toHaveCount(1);
 await group.press('Enter');await expect(page).toHaveURL(/group=layout-other-work/);await fit(page);
 await expect(page.locator('.run-node[data-session-id="layout-parent"]')).toHaveCount(1);
 await node(page,'layout-other-work').dblclick();await expect(page).toHaveURL(/view=conversation/);await expect(page).toHaveURL(/run=layout-parent/);await expect(page).toHaveURL(/group=layout-other-work/);
 await page.getByRole('button',{name:'그룹 연결',exact:true}).click();
 const chatGroups=page.getByRole('dialog',{name:'그룹 연결',exact:true});await expect(chatGroups.getByRole('checkbox',{name:'다른 업무',exact:true})).toBeChecked();
 const chatLabels=await chatGroups.locator('.group-options label>span').evaluateAll(labels=>labels.map(e=>({width:e.getBoundingClientRect().width,inside:e.getBoundingClientRect().right<=e.parentElement!.getBoundingClientRect().right})));
 for(const label of chatLabels){expect(label.width).toBeGreaterThan(30);expect(label.inside).toBe(true);}
 await chatGroups.getByRole('button',{name:'취소',exact:true}).click();
 await page.getByRole('textbox',{name:'메시지',exact:true}).fill('공유 그룹에서 같은 세션에 전송');await page.getByRole('button',{name:'전송',exact:true}).click();
 await expect(page.getByText('모의 전송 확인',{exact:true})).toBeVisible();
 const submit=wire.calls.find(c=>c.type==='submit')!.request;
 expect(submit.mode).toBe('continue');expect(submit.target_run_id).toBe('layout-parent');expect(submit.work_id).toBe('layout-work');
 await page.reload();await expect(page).toHaveURL(/run=layout-parent/);expect(wire.errors).toEqual([]);
});

test('removing every visual membership keeps the session in ungrouped after reload',async({page,baseURL},info)=>{
 const wire=await fixture(page,baseURL!,info);await page.goto('/?view=canvas&project=layout-project');await fit(page);await node(page,'layout-work').click();
 const dialog=await editor(page);await dialog.getByRole('checkbox').first().uncheck();await expect(dialog.getByText('미분류에 표시됩니다.')).toBeVisible();
 await dialog.getByRole('button',{name:'저장',exact:true}).click();await expect(dialog).toHaveCount(0);await page.reload();await fit(page);
 await expect(node(page,'ungrouped')).toBeVisible();await expect(node(page,'layout-work')).toHaveCount(0);
 expect(wire.snapshot().runs).toHaveLength(5);expect(wire.calls.map(c=>c.type)).toEqual(['set_session_groups']);expect(wire.errors).toEqual([]);
});
