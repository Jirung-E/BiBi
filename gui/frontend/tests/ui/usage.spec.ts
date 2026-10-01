import {test as base,expect,type Page} from '@playwright/test';
import {showSidebar,sidebarAction} from './navigation';
type Wire={calls:number;reads:number;wait:Promise<void>|null;error:string;completed:boolean};
const test=base.extend<{wire:Wire}>({wire:[async({page,baseURL},use)=>{
 const errors:string[]=[],wire:Wire={calls:0,reads:0,wait:null,error:'',completed:false};
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());
  if(url.origin!==new URL(baseURL!).origin){errors.push('unexpected external request');return route.abort();}
  if(req.method()==='GET'){
   if(url.pathname==='/api/snapshot'){
    wire.reads++;const response=await route.fetch(),data=await response.json();
    data.quotas=[{id:'quota',provider:'codex',provider_id:data.providers[0].id,account:'Fixture',host_id:'local',model:null,status:'known',windows:[{label:'5h',remaining_percent:wire.completed?64:78,resets_at:null}],observed_at:Date.now(),reason:null}];
    data.last_seq+=wire.completed?1:0;
    return route.fulfill({response,json:data});
   }
   return route.continue();
  }
  if(req.method()==='POST'&&url.pathname==='/api/command'&&req.postDataJSON().type==='refresh_providers'){
   wire.calls++;await wire.wait;
   if(wire.error)return route.fulfill({status:503,json:{error:wire.error}});
   wire.completed=true;return route.fulfill({json:{fixture:{status:'ok'}}});
  }
  errors.push('unexpected command');return route.abort();
 });
 await use(wire);expect(errors).toEqual([]);
},{auto:true}]});
test.beforeEach(async({page},info)=>{
 const platform=info.project.metadata.platform as string|undefined;
 if(platform)await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{value}),platform);
});
async function open(page:Page,width=1280){
 await page.setViewportSize({width,height:900});await page.goto('/?view=canvas&project=layout-project');
 await showSidebar(page);await expect(page.locator('.quota-pill-value').first()).toHaveText('78% 남음');
}
for(const width of [390,1280])test('sidebar refresh coalesces clicks and updates without leaving the canvas '+width,async({page,wire})=>{
 let release!:()=>void;wire.wait=new Promise<void>(resolve=>release=resolve);await open(page,width);
 const refresh=page.getByRole('button',{name:'사용량 새로고침',exact:true});
 try{await refresh.click();await expect(refresh).toBeDisabled();await expect(refresh).toHaveAttribute('aria-busy','true');expect(wire.calls).toBe(1);}finally{release();}
 await expect(page.locator('.quota-pill-value').first()).toHaveText('64% 남음');
 await expect(refresh).toBeEnabled();await expect(page).toHaveURL(/view=canvas/);expect(wire.reads).toBeGreaterThan(1);
 expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
});
test('usage refresh failure keeps the old value and can be retried on the detail page',async({page,wire})=>{
 await open(page);wire.error='사용량 조회 연결 실패';
 await page.getByRole('button',{name:'사용량 새로고침',exact:true}).click();
 await expect(page.locator('.sidebar-usage').getByRole('alert')).toHaveText(wire.error);
 await expect(page.locator('.quota-pill-value').first()).toHaveText('78% 남음');
 await sidebarAction(page,'사용량·연결');wire.error='';
 await page.locator('.toolbar-actions').getByRole('button',{name:'새로고침',exact:true}).click();
 await expect(page.locator('.quota-value b').first()).toHaveText('64%');
 await expect(page.getByRole('alert')).toHaveCount(0);expect(wire.calls).toBe(2);
});
