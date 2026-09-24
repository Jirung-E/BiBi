import {test as base,expect,type Page} from '@playwright/test';
import {sidebarAction} from './navigation';
import {expectOverlayScrolling} from './scrolling';
import type {ConnectionCheck} from '../../src/lib/provider-templates';
import type {ProviderConfig} from '../../src/lib/types';

type Wire={calls:ProviderConfig[];saved:ProviderConfig[];reply:ConnectionCheck;wait:Promise<void>|null};
const test=base.extend<{wire:Wire}>({
 wire:[async({page,baseURL},use)=>{
  const errors:string[]=[],wire:Wire={calls:[],saved:[],reply:{ok:true,message:'API 연결 확인 · 모델 1개',models:['gemma4:e4b']},wait:null};
  page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/*',async route=>{
   const request=route.request(),url=new URL(request.url());
   if(url.origin===new URL(baseURL!).origin){
    if(request.method()==='GET'){
     if(url.pathname==='/api/snapshot'&&wire.saved.length){const response=await route.fetch();const data=await response.json();return route.fulfill({response,json:{...data,providers:[...data.providers.filter((p:ProviderConfig)=>!wire.saved.some(s=>s.id===p.id)),...wire.saved]}});}
     return route.continue();
    }
    if(request.method()==='POST'&&url.pathname==='/api/command'){
     const body=request.postDataJSON();
     if(body.type==='save_provider'){wire.saved=wire.saved.filter(p=>p.id!==body.provider.id);wire.saved.push(structuredClone(body.provider));return route.fulfill({json:body.provider});}
     if(body.type==='check_provider'){
      wire.calls.push(body.provider);const reply=structuredClone(wire.reply);await wire.wait;
      return route.fulfill({json:reply});
     }
    }
   }
   errors.push(`unexpected request: ${request.method()} ${request.url()}`);return route.abort();
  });
  await use(wire);expect(errors).toEqual([]);
 },{auto:true}]
});
async function editor(page:Page,scale=1){
 await page.setViewportSize({width:390,height:844});
 await page.addInitScript(value=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale:value})),scale);
 await page.goto('/?view=usage');
 await sidebarAction(page,'설정');
 const dialog=page.getByRole('dialog',{name:'설정',exact:true});
 await dialog.getByRole('button',{name:'제공자 추가',exact:true}).click();
 return dialog.locator('.provider-editor');
}
for(const scale of [1,2])test(`editable templates fit mobile at ${scale*100}% without registering providers`,async({page,wire})=>{
 const form=await editor(page,scale),templates=form.getByRole('group',{name:'제공자 템플릿'});
 for(const [name,adapter,command] of [['Codex','codex','codex'],['Claude Code','claude','claude']]){
  await templates.getByRole('button',{name,exact:true}).click();
  await expect(form.getByLabel('이름',{exact:true})).toHaveValue(name);
  await expect(form.getByRole('combobox',{name:'연결 방식',exact:true})).toHaveValue(adapter);
  await expect(form.getByLabel('실행 파일',{exact:true})).toHaveValue(command);
 }
 await templates.getByRole('button',{name:'Ollama',exact:true}).click();
 await expect(form.getByLabel('API 주소',{exact:true})).toHaveValue('http://127.0.0.1:11434');
 await expect(form.getByLabel('모델 목록 · 한 줄에 하나',{exact:true})).toHaveValue('');
 await form.getByLabel('이름',{exact:true}).fill('내 서버');
 await form.getByLabel('API 주소',{exact:true}).fill('http://remote-host:11434');
 await form.getByRole('button',{name:'연결 확인',exact:true}).scrollIntoViewIfNeeded();
 await expect(form.getByRole('button',{name:'연결 확인',exact:true})).toBeInViewport();
 const dialog=page.getByRole('dialog',{name:'설정',exact:true});
 expect(await dialog.evaluate(e=>e.scrollWidth-e.clientWidth)).toBeLessThanOrEqual(1);
 expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
 await templates.getByRole('button',{name:'직접 설정',exact:true}).click();
 await expect(form.getByLabel('이름',{exact:true})).toHaveValue('');
 await expect(form.getByRole('combobox',{name:'연결 방식',exact:true})).toHaveValue('');
 expect(wire.calls).toHaveLength(0);
});
test('probe uses edited draft and model import is explicit; edits clear the result',async({page,wire})=>{
 const form=await editor(page);
 wire.reply={ok:true,message:'API 연결 확인 · 모델 24개',models:['gemma4:e4b',...Array.from({length:23},(_,i)=>'fixture:model-'+i)]};
 await form.getByRole('button',{name:'Ollama',exact:true}).click();
 await form.getByLabel('API 주소',{exact:true}).fill('http://my-ollama:11434');
 await form.getByRole('button',{name:'연결 확인',exact:true}).click();
 await expect(form.getByRole('status')).toHaveText(wire.reply.message);
 expect(wire.calls[0].endpoint).toBe('http://my-ollama:11434');
 await expect(form.getByLabel('모델 목록 · 한 줄에 하나',{exact:true})).toHaveValue('');
 await form.getByText('조회한 모델 24개',{exact:true}).click();
 const list=form.getByRole('list',{name:'조회한 모델',exact:true});
 expect(await list.evaluate(e=>e.scrollHeight>e.clientHeight)).toBe(true);
 await list.locator('li').last().scrollIntoViewIfNeeded();
 await expect(list.locator('li').last()).toBeInViewport();
 await expectOverlayScrolling(page);
 await form.getByRole('button',{name:'모델 목록에 적용',exact:true}).click();
 await expect(form.getByLabel('모델 목록 · 한 줄에 하나',{exact:true})).toHaveValue(wire.reply.models.join('\n'));
 await form.getByLabel('API 주소',{exact:true}).fill('http://another-host:11434');
 await expect(form.getByRole('status')).toHaveCount(0);
 wire.reply={ok:false,message:'API에 연결할 수 없습니다.',models:[]};
 await form.getByRole('button',{name:'연결 확인',exact:true}).click();
 await expect(form.getByRole('status')).toHaveText(wire.reply.message);
 await expect(form.getByLabel('API 주소',{exact:true})).toHaveValue('http://another-host:11434');
 await expect(form.getByRole('button',{name:'저장',exact:true})).toBeEnabled();
});
test('pending probes cannot approve changed drafts and generic commands never run',async({page,wire})=>{
 const form=await editor(page);await form.getByRole('button',{name:'Ollama',exact:true}).click();
 let release!:()=>void;wire.wait=new Promise<void>(resolve=>release=resolve);
 await form.getByRole('button',{name:'연결 확인',exact:true}).click();
 await expect(form.getByRole('button',{name:'확인 중…',exact:true})).toBeDisabled();
 await expect(form.getByRole('button',{name:'저장',exact:true})).toBeDisabled();
 await expect.poll(()=>wire.calls.length).toBe(1);
 await form.getByLabel('API 주소',{exact:true}).fill('http://different:11434');
 release();
 await expect(form.getByRole('button',{name:'연결 확인',exact:true})).toBeEnabled();
 await expect(form.getByRole('status')).toHaveCount(0);
 await form.getByRole('combobox',{name:'연결 방식',exact:true}).selectOption('command');
 await expect(form.getByRole('button',{name:'연결 확인',exact:true})).toHaveCount(0);
 await expect(form.getByText('이 연결 방식은 연결 확인을 지원하지 않습니다.',{exact:true})).toBeVisible();
 expect(wire.calls).toHaveLength(1);
});

for(const scale of [1,2])test(`Ollama generation settings save, reopen, reset and fit at ${scale*100}%`,async({page,wire})=>{
 let form=await editor(page,scale);
 await form.getByRole('button',{name:'Ollama',exact:true}).click();
 await form.getByLabel('이름',{exact:true}).fill('생성 설정 시험');
 await form.getByText('생성 설정',{exact:true}).click();
 await expect(form.getByRole('combobox',{name:'추론 모드',exact:true})).toHaveValue('');
 await expect(form.getByLabel('출력 토큰 한도',{exact:true})).toHaveValue('');
 await form.getByRole('combobox',{name:'추론 모드',exact:true}).selectOption('false');
 await form.getByLabel('출력 토큰 한도',{exact:true}).fill('0');
 await form.getByRole('button',{name:'저장',exact:true}).click();
 await expect(page.getByRole('alert')).toContainText('출력 한도');
 expect(wire.saved).toHaveLength(0);
 await form.getByLabel('출력 토큰 한도',{exact:true}).fill('2048');
 await form.getByLabel('컨텍스트 토큰 수',{exact:true}).fill('8192');
 await form.getByRole('button',{name:'저장',exact:true}).scrollIntoViewIfNeeded();
 await expect(form.getByRole('button',{name:'저장',exact:true})).toBeInViewport();
 expect(await page.getByRole('dialog',{name:'설정',exact:true}).evaluate(e=>e.scrollWidth-e.clientWidth)).toBeLessThanOrEqual(1);
 expect(await page.evaluate(()=>document.documentElement.scrollHeight-innerHeight)).toBeLessThanOrEqual(1);
 await form.getByRole('button',{name:'저장',exact:true}).click();
 await expect(form).toHaveCount(0);
 expect(wire.saved[0].ollama).toEqual({think:false,num_predict:2048,num_ctx:8192});
 const row=page.getByRole('dialog',{name:'설정',exact:true}).locator('.provider-row').filter({has:page.getByText('생성 설정 시험',{exact:true})});
 await row.getByRole('button',{name:'편집',exact:true}).click();
 form=page.locator('.provider-editor');
 await form.getByText('생성 설정',{exact:true}).click();
 await expect(form.getByRole('combobox',{name:'추론 모드',exact:true})).toHaveValue('false');
 await expect(form.getByLabel('출력 토큰 한도',{exact:true})).toHaveValue('2048');
 await expect(form.getByLabel('컨텍스트 토큰 수',{exact:true})).toHaveValue('8192');
 await form.getByRole('combobox',{name:'추론 모드',exact:true}).selectOption('');
 await form.getByLabel('출력 토큰 한도',{exact:true}).fill('');
 await form.getByLabel('컨텍스트 토큰 수',{exact:true}).fill('');
 await form.getByRole('button',{name:'저장',exact:true}).click();
 await expect(form).toHaveCount(0);
 expect(wire.saved[0].ollama).toBeNull();
});
