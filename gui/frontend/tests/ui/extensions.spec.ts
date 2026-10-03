import {test as base,expect} from '@playwright/test';
import {sidebarAction} from './navigation';
type Wire={calls:any[];status:number;entries:any[]};
const test=base.extend<{wire:Wire}>({wire:[async({page},use)=>{
 const wire:Wire={calls:[],status:200,entries:[{kind:'skill',id:'/fixture/.claude/skills/local/SKILL.md',name:'local',scope:'project',source:'/fixture/.claude/skills/local/SKILL.md',enabled:true,editable:true,removable:true,toggleable:true,content:'---\nname: local\n---\n기존 내용'},{kind:'plugin',id:'shared@example',name:'shared',scope:'inherited',source:'사용자 설정',enabled:true,editable:false,removable:false,toggleable:true},{kind:'mcp',id:'example',name:'example',scope:'project',source:'/fixture/.mcp.json',enabled:true,editable:true,removable:true,toggleable:true,config:{url:'https://example.invalid/mcp',headers:{Authorization:'__BIBI_KEEP_SECRET__'}}}]};
 const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/api/snapshot',async route=>{const data=await(await route.fetch()).json();data.providers=[{...data.providers[0],adapter:'claude',name:'Claude fixture'}];await route.fulfill({json:data});});
 await page.route('**/api/command',async route=>{
  const body=route.request().postDataJSON();wire.calls.push(body);
  if(body.type==='list_project_extensions')return route.fulfill({json:{supported:true,workspace:'/fixture/project',host_id:'local',entries:wire.entries,revision:'revision-1',notes:['다음 메시지부터 적용'],errors:[],can_install_plugin:true}});
  if(body.type==='update_project_extension'){
   if(wire.status!==200)return route.fulfill({status:wire.status,json:{error:'다른 곳에서 설정이 변경되었습니다. 새로고침 후 다시 저장하세요.'}});
   const e=body.edit,old=wire.entries.find(x=>x.id===e.id&&x.kind===e.kind);
   if(e.action==='toggle')old.enabled=e.value;
   if(e.action==='save'){if(e.kind==='skill')old.content=e.value;else old.config=e.value;}
   if(e.action==='remove')wire.entries=wire.entries.filter(x=>x!==old);
   return route.fulfill({json:{saved:true,apply:'next_turn'}});
  }
  if(body.type==='check_project_mcp')return route.fulfill({json:{connected:true,tools:[{name:'read_fixture',description:'검사용 도구'}]}});
  return route.fulfill({status:422,json:{error:'unexpected command'}});
 });
 await use(wire);expect(errors).toEqual([]);
},{auto:true}]});
for(const [width,scale] of [[1280,1],[390,1],[320,2]])test(`project extension controls preserve scope at ${width}/${scale}`,async({page,wire},info)=>{
 await page.setViewportSize({width,height:844});await page.emulateMedia({colorScheme:'dark'});
 if(info.project.metadata.platform)await page.addInitScript(p=>Object.defineProperty(navigator,'platform',{value:p}),info.project.metadata.platform);
 await page.addInitScript(uiScale=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale})),scale);
 await page.goto('/?project=layout-project');await sidebarAction(page,'프로젝트 확장 설정');
 const dialog=page.getByRole('dialog',{name:'프로젝트 확장',exact:true});await dialog.getByLabel('확장 제공자').selectOption('layout-provider');
 await dialog.getByRole('button',{name:'local 편집',exact:true}).click();await dialog.getByRole('textbox',{name:'스킬 내용',exact:true}).fill('수정된 스킬');
 wire.status=409;await dialog.getByRole('button',{name:'저장',exact:true}).click();await expect(dialog.getByRole('alert')).toContainText('다른 곳');await expect(dialog.getByRole('textbox',{name:'스킬 내용',exact:true})).toHaveValue('수정된 스킬');
 wire.status=200;await dialog.getByRole('button',{name:'저장',exact:true}).click();await expect(dialog.getByRole('status')).toContainText('다음 메시지');
 await dialog.getByRole('tab',{name:'플러그인',exact:true}).click();await expect(dialog.getByRole('button',{name:'shared 제거',exact:true})).toHaveCount(0);await dialog.getByRole('button',{name:'shared 끄기',exact:true}).click();await expect(dialog.getByRole('button',{name:'shared 사용',exact:true})).toBeVisible();
 await dialog.getByRole('tab',{name:'MCP',exact:true}).click();await dialog.getByRole('button',{name:'example 편집',exact:true}).click();await expect(dialog.getByRole('textbox',{name:'MCP 설정',exact:true})).toHaveValue(/__BIBI_KEEP_SECRET__/);await dialog.getByRole('button',{name:'취소',exact:true}).click();
 await dialog.getByRole('button',{name:'example 연결 확인',exact:true}).click();await expect(dialog.getByText('read_fixture',{exact:true})).toBeVisible();
 await dialog.getByRole('button',{name:'example 제거',exact:true}).click();await dialog.getByRole('button',{name:'제거 확인',exact:true}).click();await expect(dialog.getByText('등록된 MCP 없음',{exact:true})).toBeVisible();
 expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);expect(await dialog.evaluate(e=>e.scrollWidth-e.clientWidth)).toBeLessThanOrEqual(1);
 expect(wire.calls.filter(c=>c.type==='update_project_extension').every(c=>c.project_key==='layout-project'&&c.provider_id==='layout-provider'&&c.edit.revision==='revision-1')).toBe(true);
 expect(wire.calls.some(c=>c.type==='submit')).toBe(false);
 await dialog.getByRole('button',{name:'MCP 추가',exact:true}).click();await dialog.getByLabel('확장 이름').fill('new-server');await dialog.getByRole('textbox',{name:'MCP 설정',exact:true}).fill('{"command":"example","args":[]}');await dialog.getByRole('button',{name:'저장',exact:true}).click();
 await page.screenshot({path:info.outputPath('extensions-'+width+'-'+scale+'.png')});
});
test('unsupported provider and failed lookup are explicit, not empty successful lists',async({page})=>{
 await page.route('**/api/command',route=>route.fulfill({status:422,json:{error:'원격 호스트 연결 실패'}}));
 await page.goto('/?modal=extensions&project=layout-project');await page.getByLabel('확장 제공자').selectOption('layout-provider');await expect(page.getByRole('alert')).toContainText('원격 호스트');await expect(page.getByRole('button',{name:'스킬 추가',exact:true})).toHaveCount(0);
});
