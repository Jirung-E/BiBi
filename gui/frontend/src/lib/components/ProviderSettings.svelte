<script lang="ts">
import {command} from '../api';
import {providers as adapters} from '../format';
import {submissionId} from '../drafts';
import type {ProviderConfig,Provider} from '../types';
let {providers,onchanged}:{providers:ProviderConfig[];onchanged:()=>Promise<void>}=$props();
let editing=$state<ProviderConfig|null>(null),modelText=$state(''),argsText=$state(''),apiKey=$state(''),clearKey=$state(false),error=$state(''),busy=$state(false),deleting=$state('');
function edit(p?:ProviderConfig){editing=p?{...p,args:[...p.args],models:[...p.models]}:{id:submissionId().replace('submission_','provider_'),name:'',host_id:'local',remote_id:null,adapter:'' as Provider,command:'',args:[],endpoint:'',models:[],api_key_set:false};modelText=p?.models.join('\n')??'';argsText=p?JSON.stringify(p.args):'[]';apiKey='';clearKey=false;error='';}
async function save(){if(!editing||busy)return;busy=true;error='';try{const args=JSON.parse(argsText);if(!Array.isArray(args)||args.some(a=>typeof a!=='string'))throw new Error('인자는 문자열 배열로 입력하세요.');await command({type:'save_provider',provider:{...editing,args,models:modelText.split('\n')},api_key:clearKey?'':apiKey||null});apiKey='';editing=null;await onchanged();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
async function remove(id:string){busy=true;error='';try{await command({type:'delete_provider',provider_id:id});deleting='';await onchanged();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
</script>
<section class="provider-settings">
 <div class="row"><h2>모델 제공자</h2><button onclick={()=>edit()} disabled={busy}>+ 제공자 추가</button></div>
 {#if !providers.length&&!editing}<p class="muted">등록된 제공자 없음</p>{/if}
 {#each providers.filter(p=>p.host_id==='local') as p(p.id)}<div class="provider-row"><div><strong>{p.name}</strong><small>{adapters[p.adapter]}</small></div><button onclick={()=>edit(p)} disabled={busy}>편집</button><button class="danger-button" onclick={()=>deleting=p.id} disabled={busy}>제거</button>
 {#if deleting===p.id}<div class="inline-confirm"><span>{p.name} 연결을 제거합니다. 대화 기록은 보존됩니다.</span><button class="danger-button" onclick={()=>remove(p.id)} disabled={busy}>제거 확인</button><button onclick={()=>deleting=''}>취소</button></div>{/if}</div>{/each}
 {#each providers.filter(p=>p.host_id!=='local') as remote}<div class="provider-row"><div><strong>{remote.name}</strong><small>{remote.host_id} · 이 호스트에서 편집</small></div></div>{/each}
 {#if editing}<form class="provider-editor" onsubmit={(e)=>{e.preventDefault();void save();}}>
  <label>이름<input bind:value={editing.name} required maxlength="160" placeholder="예: 내 Antigravity" /></label>
  <label>연결 방식<select bind:value={editing.adapter} required><option value="" disabled>선택</option>{#each Object.entries(adapters) as [key,label]}<option value={key}>{label}</option>{/each}</select></label>
  {#if ['codex','claude','command'].includes(editing.adapter)}<label>실행 파일<input bind:value={editing.command} required placeholder="명령 또는 전체 경로" /></label><label>실행 인자 · JSON 배열<textarea rows="2" bind:value={argsText} placeholder='["--flag", "value"]'></textarea></label>{/if}
  {#if ['open_ai','ollama'].includes(editing.adapter)}<label>API 주소<input type="url" bind:value={editing.endpoint} required placeholder="https://example.com/v1" /></label>{/if}
  {#if editing.adapter==='open_ai'}<label>API 키<input type="password" autocomplete="new-password" bind:value={apiKey} placeholder={editing.api_key_set?'저장됨 · 비우면 기존 키 유지':'선택'} /></label>{#if editing.api_key_set}<label class="check"><input type="checkbox" bind:checked={clearKey} />저장된 키 제거</label>{/if}<small>Chat Completions 호환 API · 텍스트 대화</small>{/if}
  {#if editing.adapter==='command'}<small>비대화형 명령 · JSON 표준 입력 / 텍스트 표준 출력. 인자 치환은 아래 도움말을 참조하세요.</small><details><summary>입출력 형식</summary><code>{'{prompt} · {model} · {workspace} · {session_id}'}</code><p>표준 입력: model, prompt, messages, session_id, workspace를 포함한 JSON 한 줄. 표준 출력: 답변 텍스트. 각 차례에 새 프로세스를 실행하며 messages로 대화 기록을 전달합니다.</p></details>{/if}
  <label>모델 목록 · 한 줄에 하나<textarea rows="3" bind:value={modelText} placeholder="선택"></textarea></label>
  <div class="row"><button type="button" onclick={()=>editing=null} disabled={busy}>취소</button><button class="primary" disabled={busy||!editing.adapter}>{busy?'저장 중…':'저장'}</button></div>
 </form>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>
