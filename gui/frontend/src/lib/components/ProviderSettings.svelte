<script lang="ts">
import {reveal,disclosure} from '../motion';
import Icon from './Icon.svelte';
import {scrollbars} from '../scrollbars';
import {command} from '../api';
import {providers as adapters} from '../format';
import {submissionId} from '../drafts';
import {providerTemplates,type ProviderTemplate,type ConnectionCheck} from '../provider-templates';
import type {ProviderConfig,Provider} from '../types';
let {providers,onchanged}:{providers:ProviderConfig[];onchanged:()=>Promise<void>}=$props();
let editing=$state<ProviderConfig|null>(null),modelText=$state(''),argsText=$state(''),apiKey=$state(''),clearKey=$state(false),error=$state(''),busy=$state(false),deleting=$state('');
let isNew=$state(false),checking=$state(false),checkResult=$state<ConnectionCheck|null>(null);
let thinking=$state(''),numPredict=$state<number|null>(null),numCtx=$state<number|null>(null);
let requestId=0,checkVersion=0;
const checkSignature=$derived(JSON.stringify([editing?.id,editing?.host_id,editing?.adapter,editing?.command,editing?.endpoint,argsText,apiKey,clearKey,thinking,numPredict,numCtx]));
const checkSupported=$derived(!!editing&&['codex','claude','ollama','open_ai'].includes(editing.adapter));
$effect(()=>{checkSignature;checkVersion+=1;checkResult=null;});
function resetCheck(){requestId+=1;checking=false;checkResult=null;}
function generation(p?:ProviderConfig){thinking=p?.ollama?.think===undefined||p?.ollama?.think===null?'':String(p.ollama.think);numPredict=p?.ollama?.num_predict??null;numCtx=p?.ollama?.num_ctx??null;}
function edit(p?:ProviderConfig){resetCheck();generation(p);isNew=!p;editing=p?{...p,args:[...p.args],models:[...p.models]}:{id:submissionId().replace('submission_','provider_'),name:'',host_id:'local',remote_id:null,adapter:'' as Provider,command:'',args:[],endpoint:'',models:[],api_key_set:false};modelText=p?.models.join('\n')??'';argsText=p?JSON.stringify(p.args):'[]';apiKey='';clearKey=false;error='';}
function template(value:ProviderTemplate){if(!editing)return;resetCheck();generation();editing={...editing,...value,args:[],models:[],api_key_set:false,ollama:null};argsText='[]';modelText='';apiKey='';clearKey=false;error='';}
function draft(){
 if(!editing)throw new Error('제공자를 선택하세요.');
 const args=JSON.parse(argsText);
 if(!Array.isArray(args)||args.some(a=>typeof a!=='string'))throw new Error('인자는 문자열 배열로 입력하세요.');
 if(editing.adapter==='ollama'&&((numPredict!==null&&(!Number.isInteger(numPredict)||(numPredict!==-1&&numPredict<1)))||(numCtx!==null&&(!Number.isInteger(numCtx)||numCtx<1))))throw new Error('출력 한도는 양의 정수 또는 -1, 컨텍스트는 양의 정수로 입력하세요.');
 const ollama=editing.adapter==='ollama'&&(thinking!==''||numPredict!==null||numCtx!==null)?{...(thinking!==''?{think:thinking==='true'}:{}),...(numPredict!==null?{num_predict:numPredict}:{}),...(numCtx!==null?{num_ctx:numCtx}:{})}:null;
 return {...editing,args,models:modelText.split('\n'),ollama};
}
async function checkConnection(){
 if(!editing||busy||checking||!checkSupported)return;
 const id=++requestId,version=checkVersion,signature=checkSignature;
 checking=true;checkResult=null;
 try{
  const result=await command<ConnectionCheck>({type:'check_provider',provider:draft(),api_key:clearKey?'':apiKey||null});
  if(id===requestId&&version===checkVersion&&signature===checkSignature)checkResult=result;
 }catch(e){if(id===requestId&&version===checkVersion&&signature===checkSignature)checkResult={ok:false,message:e instanceof Error?e.message:String(e),models:[]};}
 finally{if(id===requestId)checking=false;}
}
async function save(){if(!editing||busy||checking)return;busy=true;error='';try{await command({type:'save_provider',provider:draft(),api_key:clearKey?'':apiKey||null});apiKey='';resetCheck();editing=null;await onchanged();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
async function remove(id:string){busy=true;error='';try{await command({type:'delete_provider',provider_id:id});deleting='';await onchanged();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
function cancel(){resetCheck();editing=null;apiKey='';error='';}
</script>
<section class="provider-settings">
 <div class="row"><h2>모델 제공자</h2><button onclick={()=>edit()} disabled={busy}><Icon name="plus" /><span>제공자 추가</span></button></div>
 {#if !providers.length&&!editing}<p class="muted">등록된 제공자 없음</p>{/if}
 {#each providers.filter(p=>p.host_id==='local') as p(p.id)}<div class="provider-row"><div><strong>{p.name}</strong><small>{adapters[p.adapter]}</small></div><button onclick={()=>edit(p)} disabled={busy}>편집</button><button class="danger-button" onclick={()=>deleting=p.id} disabled={busy}>제거</button>
 {#if deleting===p.id}<div transition:reveal class="inline-confirm"><span>{p.name} 연결을 제거합니다. 대화 기록은 보존됩니다.</span><div class="form-actions"><button onclick={()=>deleting=''}>취소</button><button class="danger-button" onclick={()=>remove(p.id)} disabled={busy}>제거 확인</button></div></div>{/if}</div>{/each}
 {#each providers.filter(p=>p.host_id!=='local') as remote}<div class="provider-row"><div><strong>{remote.name}</strong><small>{remote.host_id} · 이 호스트에서 편집</small></div></div>{/each}
 {#if editing}<form transition:reveal class="provider-editor" onsubmit={(e)=>{e.preventDefault();void save();}}>
  {#if isNew}<div class="provider-templates" role="group" aria-label="제공자 템플릿"><span>템플릿</span>{#each providerTemplates as item}<button type="button" onclick={()=>template(item)} disabled={busy}>{item.name}</button>{/each}<button type="button" onclick={()=>edit()} disabled={busy}>직접 설정</button></div>{/if}
  <label>이름<input bind:value={editing.name} required maxlength="160" placeholder="예: 내 Antigravity" /></label>
  <label>연결 방식<select bind:value={editing.adapter} required><option value="" disabled>선택</option>{#each Object.entries(adapters) as [key,label]}<option value={key}>{label}</option>{/each}</select></label>
  {#if ['codex','claude','command'].includes(editing.adapter)}<label>실행 파일<input bind:value={editing.command} required placeholder="명령 또는 전체 경로" /></label><label>실행 인자 · JSON 배열<textarea use:scrollbars rows="2" bind:value={argsText} placeholder='["--flag", "value"]'></textarea></label>{/if}
  {#if ['open_ai','ollama'].includes(editing.adapter)}<label>API 주소<input type="url" bind:value={editing.endpoint} required placeholder={editing.adapter==='ollama'?'http://127.0.0.1:11434':'https://example.com/v1'} /></label><small>BiBi 서버가 실행되는 컴퓨터에서 접속할 주소</small>{/if}
  {#if editing.adapter==='open_ai'}<label>API 키<input type="password" autocomplete="new-password" bind:value={apiKey} placeholder={editing.api_key_set?'저장됨 · 비우면 기존 키 유지':'선택'} /></label>{#if editing.api_key_set}<label class="check"><input type="checkbox" bind:checked={clearKey} />저장된 키 제거</label>{/if}<small>Chat Completions 호환 API · 텍스트 대화</small>{/if}
  {#if editing.adapter==='ollama'}<details use:disclosure><summary>생성 설정</summary>
   <label>추론 모드<select bind:value={thinking}><option value="">모델 기본</option><option value="false">끔</option><option value="true">켬</option></select></label>
   <label>출력 토큰 한도<input type="number" step="1" min="-1" bind:value={numPredict} placeholder="서버 기본" /></label><small>추론과 답변에 함께 사용 · -1은 자동 종료까지 생성</small>
   <label>컨텍스트 토큰 수<input type="number" step="1" min="1" bind:value={numCtx} placeholder="서버 기본" /></label><small>늘리면 메모리 사용량이 증가합니다.</small>
  </details>{/if}
  {#if editing.adapter==='command'}<small>비대화형 명령 · JSON 표준 입력 / 텍스트 표준 출력. 인자 치환은 아래 도움말을 참조하세요.</small><details use:disclosure><summary>입출력 형식</summary><code>{'{prompt} · {model} · {workspace} · {session_id}'}</code><p>표준 입력: model, prompt, messages, session_id, workspace를 포함한 JSON 한 줄. 표준 출력: 답변 텍스트. 각 차례에 새 프로세스를 실행하며 messages로 대화 기록을 전달합니다.</p></details>{/if}
  {#if checkSupported}<div class="provider-check">
   <button type="button" onclick={checkConnection} disabled={busy||checking}>{checking?'확인 중…':'연결 확인'}</button><small>모델 호출 없이 확인</small>
   {#if checkResult}<p class:success={checkResult.ok} class:error={!checkResult.ok} role="status">{checkResult.message}</p>
    {#if checkResult.models.length}<details use:disclosure><summary>조회한 모델 {checkResult.models.length}개</summary><ul use:scrollbars aria-label="조회한 모델">{#each checkResult.models as model}<li>{model}</li>{/each}</ul><button type="button" onclick={()=>modelText=checkResult!.models.join('\n')}>모델 목록에 적용</button></details>{/if}
   {/if}
  </div>{:else if editing.adapter}<small>이 연결 방식은 연결 확인을 지원하지 않습니다.</small>{/if}
  <label>모델 목록 · 한 줄에 하나<textarea use:scrollbars rows="3" bind:value={modelText} placeholder="선택"></textarea></label>
  <div class="form-actions"><button type="button" onclick={cancel} disabled={busy}>취소</button><button class="primary" disabled={busy||checking||!editing.adapter}>{busy?'저장 중…':'저장'}</button></div>
 </form>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>
