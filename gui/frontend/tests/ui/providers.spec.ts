import {test as base,expect,type Page} from '@playwright/test';
import {sidebarAction} from './navigation';
import {expectOverlayScrolling} from './scrolling';
import type {ConnectionCheck} from '../../src/lib/provider-templates';
import type {ProviderConfig} from '../../src/lib/types';

type Wire={calls:ProviderConfig[];reply:ConnectionCheck;wait:Promise<void>|null};
const test=base.extend<{wire:Wire}>({
 wire:[async({page,baseURL},use)=>{
  const errors:string[]=[],wire:Wire={calls:[],reply:{ok:true,message:'API 연결 확인 · 모델 1개',models:['gemma4:e4b']},wait:null};
  page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/*',async route=>{
   const request=route.request(),url=new URL(request.url());
   if(url.origin===new URL(baseURL!).origin){
    if(request.method()==='GET')return route.continue();
    if(request.method()==='POST'&&url.pathname==='/api/command'){
     const body=request.postDataJSON();
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
 await dialog.getByRole('button',{name:'+ 제공자 추가',exact:true}).click();
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
