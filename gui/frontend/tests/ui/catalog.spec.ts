import {test as base,expect,type Page} from '@playwright/test';
import type {Snapshot,Run} from '../../src/lib/types';
type Entry={id:string;title:string;provider:string;provider_id:string;provider_name:string;workspace:string;project_key:string;project_name:string;host_id:string;native_id:string;run_id:string|null;state:string;updated_at:number;excerpt:string;last_user:string;last_assistant:string;terminal_supported:boolean;terminal_reason:string|null;external:boolean};
type Wire={snapshot?:Snapshot;items:Entry[];commands:Record<string,unknown>[];queries:URL[];fail:boolean;confirm:boolean;focus:boolean;terminalError:boolean};
const item:Entry={id:'native:claude:original',title:'다른 폴더의 원본 세션',provider:'claude',provider_id:'layout-provider',provider_name:'Claude 회사',workspace:"/different/folder with 한글 ' $()",project_key:'elsewhere',project_name:'별도 프로젝트',host_id:'local',native_id:'original-native',run_id:null,state:'disconnected',updated_at:1_792_000_000_000,excerpt:'검색된 과거 내용',last_user:'원래 마지막 질문',last_assistant:'원래 마지막 답변',terminal_supported:true,terminal_reason:null,external:true};
const test=base.extend<{wire:Wire}>({wire:[async({page,baseURL},use)=>{
 const errors:string[]=[],wire:Wire={items:[{...item},{...item,id:'run:remote',title:'원격 기록',host_id:'other-host',terminal_supported:false,terminal_reason:'세션이 저장된 호스트에서 여세요.'}],commands:[],queries:[],fail:false,confirm:true,focus:false,terminalError:false};page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());if(url.origin!==new URL(baseURL!).origin){errors.push('external request');return route.abort();}
  if(url.pathname==='/api/snapshot'){wire.snapshot??=await(await route.fetch()).json();wire.snapshot!.session_catalog_v1=true;wire.snapshot!.native_session_controls_v1=true;wire.snapshot!.providers[0].adapter='claude';if(!wire.snapshot!.projects.some(p=>p.id==='elsewhere'))wire.snapshot!.projects.push({id:'elsewhere',name:'별도 프로젝트',workspace:item.workspace,guild_path:null,constraints:[]});return route.fulfill({json:wire.snapshot});}
  if(url.pathname==='/api/sessions/catalog'){wire.queries.push(url);if(wire.fail)return route.fulfill({status:503,json:{error:'조회 실패'}});const q=url.searchParams.get('q')??'';return route.fulfill({json:{items:q==='없음'?[]:wire.items,warnings:[],has_more:false}});}
  if(url.pathname==='/api/command'&&req.method()==='POST'){
   const body=req.postDataJSON();wire.commands.push(body);
   if(body.type==='check_terminal')return route.fulfill({json:{action:wire.focus?'focus':'resume',requires_confirmation:wire.confirm,entry:item}});
   if(body.type==='open_terminal')return wire.terminalError?route.fulfill({status:409,json:{error:'원래 세션이 다시 열렸습니다.'}}):route.fulfill({json:{status:'requested',message:wire.focus?'원래 터미널 탭을 열었습니다.':'이 호스트의 터미널에 같은 세션 열기를 요청했습니다.'}});
   if(body.type==='open_catalog_session'){const original=wire.snapshot!.runs[0],r={...original,id:'imported',session_id:'imported',project_key:'elsewhere',workspace:item.workspace,title:item.title,session_key:item.native_id,provider:'claude',origin:'external',state:'disconnected'} as Run;wire.snapshot!.runs.push(r);return route.fulfill({json:r});}
   errors.push('unexpected '+body.type);return route.abort();
  }
  if(url.pathname.startsWith('/api/runs/imported')){const r=wire.snapshot!.runs.find(r=>r.id==='imported');return route.fulfill({json:url.pathname.endsWith('/status')?r:{run:r,messages:[],conversation:[],inbox:[],approvals:[],inputs:[]}});}
  return route.continue();
 });
 await use(wire);expect(errors).toEqual([]);
},{auto:true}]});
test.beforeEach(async({page},info)=>{if(info.project.metadata.platform)await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{value:'Win32'}));});
async function open(page:Page,width=1280){await page.setViewportSize({width,height:844});await page.goto('/?view=canvas&project=layout-project');await expect(page.locator('.board')).toBeVisible();await page.keyboard.press('Control+k');const dialog=page.getByRole('dialog',{name:'세션 검색',exact:true});await expect(dialog.locator('.catalog-result')).toHaveCount(2);return dialog;}
for(const width of [390,1280])test('global search previews other folders and opens the same conversation at '+width,async({page,wire},info)=>{
 const dialog=await open(page,width);const search=dialog.getByRole('searchbox');await expect(search).toBeFocused();await expect(dialog.getByLabel('검색 프로젝트')).toHaveValue('');await expect(dialog.getByLabel('선택한 세션 미리보기')).toContainText('원래 마지막 답변');expect(wire.commands).toHaveLength(0);
 await dialog.getByLabel('검색 프로젝트').selectOption('elsewhere');await dialog.getByLabel('검색 제공자').selectOption('layout-provider');await search.fill('folder');await expect.poll(()=>wire.queries.at(-1)?.searchParams.get('q')).toBe('folder');await expect(dialog.locator('.catalog-result').first()).toBeEnabled();expect(wire.queries.at(-1)?.searchParams.get('project')).toBe('elsewhere');expect(wire.queries.at(-1)?.searchParams.get('provider')).toBe('layout-provider');
 const box=(await dialog.boundingBox())!;expect(box.x).toBeGreaterThanOrEqual(0);expect(box.x+box.width).toBeLessThanOrEqual(width+1);expect(box.y+box.height).toBeLessThanOrEqual(845);
 if(width===1280&&info.project.name==='chromium')await page.screenshot({path:info.outputPath('global-session-search.png')});
 await search.press('Enter');await expect(dialog).toHaveCount(0);await expect(page).toHaveURL(/project=elsewhere/);await expect(page).toHaveURL(/run=imported/);expect(wire.commands).toEqual([{type:'open_catalog_session',id:item.id}]);await page.goBack();await expect(dialog).toBeVisible();
});
test('keyboard selection reveals unsupported remote target without opening a local terminal',async({page,wire})=>{
 const dialog=await open(page);await dialog.getByRole('searchbox').press('ArrowDown');await expect(dialog.getByLabel('선택한 세션 미리보기')).toContainText('세션이 저장된 호스트');await expect(dialog.getByRole('button',{name:'터미널에서 열기',exact:true})).toHaveCount(0);expect(wire.commands).toHaveLength(0);
});
test('terminal action requires confirmation and shows errors before a retry',async({page,wire})=>{
 const dialog=await open(page,390);await dialog.getByRole('button',{name:'터미널에서 열기',exact:true}).click();const launch=dialog.getByRole('button',{name:'터미널에서 열기',exact:true});await expect(launch).toBeDisabled();expect(wire.commands.map(c=>c.type)).toEqual(['check_terminal']);await dialog.getByRole('checkbox').check();wire.terminalError=true;await launch.click();await expect(dialog.getByRole('alert')).toHaveText('원래 세션이 다시 열렸습니다.');wire.terminalError=false;await launch.click();await expect(dialog.getByRole('status').last()).toContainText('이 호스트의 터미널');expect(wire.commands.at(-1)).toEqual({type:'open_terminal',id:item.id,confirmed_stopped:true});
});
test('verified existing terminal focuses immediately and search recovers from failure',async({page,wire})=>{
 wire.confirm=false;wire.focus=true;const dialog=await open(page);await dialog.getByRole('button',{name:'터미널에서 열기',exact:true}).click();await expect(dialog.getByRole('status').last()).toContainText('원래 터미널 탭');expect(wire.commands.at(-1)).toEqual({type:'open_terminal',id:item.id,confirmed_stopped:false});wire.fail=true;await dialog.getByRole('searchbox').fill('오류');await expect(dialog.getByRole('alert')).toHaveText('조회 실패');wire.fail=false;await dialog.getByRole('searchbox').fill('없음');await expect(dialog.getByText('일치하는 세션이 없습니다.')).toBeVisible();
});
