import {test,expect} from '@playwright/test';
const entries=[{name:'rename',description:'제공자 이름 변경',source:'provider',supported:true},{name:'terminal-setup',description:'터미널 설정',source:'provider',supported:false,reason:'원본 CLI에서 실행하세요.'},{name:'bibi rename',description:'BiBi 이름 변경',source:'bibi',supported:true},...Array.from({length:24},(_,i)=>({name:'skill-'+i,description:'프로젝트 명령 '+i,source:'skill',supported:true}))];
test('provider commands do not collide with BiBi commands and unsupported commands never become prompts',async({page})=>{
 const calls:any[]=[],errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/api/snapshot',async route=>{const data=await(await route.fetch()).json();data.providers[0].adapter='claude';for(const r of data.runs){r.provider='claude';r.read_only=false;r.runtime.commands=[{name:'rename',description:'제공자 명령',argument_hint:''}];}await route.fulfill({json:data});});
 await page.route('**/api/command',async route=>{const body=route.request().postDataJSON();calls.push(body);if(body.type==='list_commands')return route.fulfill({json:{entries,errors:[]}});if(body.type==='submit')return route.fulfill({status:422,json:{error:'fixture accepted shape'}});return route.fulfill({json:{}});});
 await page.goto('/?view=conversation&project=layout-project&run=layout-parent');const input=page.getByRole('textbox',{name:'메시지',exact:true});
 await input.fill('/');await expect(page.getByLabel('슬래시 명령').getByRole('button')).toHaveCount(27);await expect(page.getByRole('button',{name:/terminal-setup/})).toBeDisabled();
 await input.fill('/rename native title');await page.getByRole('button',{name:'전송',exact:true}).click();expect(calls.filter(c=>c.type==='submit').at(-1).request.question).toBe('/rename native title');expect(calls.some(c=>c.type==='rename_session')).toBe(false);
 await input.fill('/not-found');await page.getByRole('button',{name:'전송',exact:true}).click();await expect(page.getByRole('alert')).toContainText('지원하지 않는 명령');expect(calls.filter(c=>c.type==='submit')).toHaveLength(1);
 await input.fill('/bibi model selected-fixture');await page.getByRole('button',{name:'전송',exact:true}).click();await expect(page.getByLabel('모델',{exact:true})).toHaveValue('selected-fixture');expect(calls.filter(c=>c.type==='submit')).toHaveLength(1);
 await input.fill('/bibi extensions');await page.getByRole('button',{name:'전송',exact:true}).click();await expect(page.getByRole('dialog',{name:'프로젝트 확장',exact:true})).toBeVisible();expect(errors).toEqual([]);
});
