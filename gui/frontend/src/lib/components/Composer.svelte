<script lang="ts">
import {scrollbars} from '../scrollbars';
import type {Project,Run,Work,Host,Submission,Receipt,ProviderConfig,ModelHistory,ModelSelection} from '../types';
import {command,request,ApiError} from '../api';
import {draftKey,loadDraft,saveDraft,submissionId} from '../drafts';
import {providerName,shortId,isActive} from '../format';
import {modelSuggestions,recentModels} from '../models';
import {sessionId} from '../sessions';
let {serverId,project,run=null,work=null,hosts,providers,modelHistory,selection,onaccepted,onsettings,onlocal}: {serverId:string;project:Project;run?:Run|null;work?:Work|null;hosts:Host[];providers:ProviderConfig[];modelHistory:ModelHistory[];selection:ModelSelection|null;onaccepted:(receipt:Receipt)=>void;onsettings:()=>void;onlocal:(name:string,arg:string)=>Promise<void>} = $props();
let text=$state(''),pending=$state<Submission|null>(null),sending=$state(false),error=$state(''),mode=$state<'continue'|'fresh'|'steer'>('fresh');
let providerId=$state(''),model=$state(''),role=$state('업무 조정'),host=$state('local'),readOnly=$state(false);
let loadedKey='';
const busySession=$derived(!!run&&(isActive(run.state)||['queued','uncertain','disconnected'].includes(run.state)));
const key=$derived(draftKey(serverId,project.id,work?.id??'new',sessionId(run??undefined)||'new'));
const available=$derived(mode==='fresh'?providers:providers.filter(p=>p.adapter===run?.provider&&p.host_id===run?.host_id));
const provider=$derived(providers.find(p=>p.id===providerId));
const suggestions=$derived(modelSuggestions(provider,modelHistory));
const recent=$derived(recentModels(providerId,modelHistory));
const slash=$derived(text.startsWith('/')&&!text.includes(' ')&&text.length<50?text.slice(1):null);
const slashOptions=$derived([...['new','clear','rename','usage'].map(name=>({name,description:({new:'새 세션',clear:'새 세션',rename:'세션 이름 변경',usage:'사용량'} as Record<string,string>)[name]})),...(run?.runtime?.commands??[]).filter(c=>!['new','clear','rename','usage'].includes(c.name))].filter(c=>slash!==null&&c.name.startsWith(slash)).slice(0,12));
$effect(()=>{
 if(key===loadedKey)return;
 loadedKey=key;const saved=loadDraft(key);text=saved.text;pending=saved.pending;error='';mode=run?.capabilities.continue_session?.supported?'continue':'fresh';
 providerId=run?.provider_id??(providers.some(p=>p.id===selection?.provider_id&&(!run||p.adapter===run.provider))?selection!.provider_id:'');model=run?.model??selection?.model??'';role=run?.role??'업무 조정';host=run?.host_id??providers.find(p=>p.id===providerId)?.host_id??'local';readOnly=run?.read_only??false;
});
$effect(()=>{if(mode!=='fresh'&&run){model=run.model;if(run.provider_id)providerId=run.provider_id;}});
function save(){try{saveDraft(key,{text,pending});}catch{error='이 브라우저에 초안을 저장할 수 없습니다.';}}
async function rememberModel(){if(!provider)return;try{await command({type:'select_model',selection:{provider_id:providerId,model:model.trim()}});}catch(e){error=e instanceof Error?e.message:String(e);}}
function providerChanged(){host=mode!=='fresh'&&run?run.host_id:provider?.host_id??'local';model=mode!=='fresh'&&run?run.model:recentModels(providerId,modelHistory)[0]?.model??'';void rememberModel();}
async function send(){
 if(sending||(!text.trim()&&!pending))return;
 const capturedKey=key;error='';sending=true;
 try{
 const match=!pending&&text.trim().match(/^\/(new|clear|rename|usage)(?:\s+([\s\S]*))?$/);
 if(match){await onlocal(match[1],match[2]??'');text='';save();return;}
 if(!pending&&!provider)throw new Error('제공자를 등록하고 선택하세요.');
 let payload:Submission=pending??{
  submission_id:submissionId(),project_key:project.id,work_id:work?.id??null,title:null,question:text,
  provider:provider!.adapter,provider_id:providerId,model:mode==='steer'&&run?run.model:model.trim(),host_id:mode!=='fresh'&&run?run.host_id:host,role:mode!=='fresh'&&run?run.role:role,mode,
  target_run_id:run?.id??null,expected_turn_id:run?.turn_id??null,expected_context_revision:work?.context_revision??null,read_only:mode!=='fresh'&&run?run.read_only:readOnly||provider?.adapter==='ollama'
 };
 let receipt:Receipt|undefined;
 if(pending){try{receipt=await request<Receipt>('/api/submissions/'+pending.submission_id);}catch(e){if(!(e instanceof ApiError)||e.status!==404)throw e;}}
 pending=payload;saveDraft(capturedKey,{text:payload.question,pending:payload});
 receipt??=await command<Receipt>({type:'submit',request:payload});
 saveDraft(capturedKey,{text:'',pending:null});if(key===capturedKey){text='';pending=null;}onaccepted(receipt);
 }catch(e){if(key===capturedKey){error=e instanceof Error?e.message:String(e);if(e instanceof ApiError&&e.status>=400&&e.status<500&&e.status!==408){pending=null;save();}else if(pending)error+=' · 접수 확인 후 같은 전송을 재시도합니다.';}}
 finally{sending=false;}
}
</script>
<form class="composer" use:scrollbars aria-label="메시지 작성" onsubmit={(e)=>{e.preventDefault();void send();}}>
 <div class="compose-target">{#if run}{providerName(run,providers)} · {run.model||'모델 확인 대기'} · {shortId(sessionId(run))}{:else}{project.name} · 새 업무{/if}</div>
 <label class="sr-only" for={'message-'+(run?.id??'new')}>메시지</label>
 <textarea use:scrollbars id={'message-'+(run?.id??'new')} bind:value={text} oninput={save} disabled={sending||pending!==null} rows="3" placeholder="메시지 입력 · / 명령"
 onkeydown={(e)=>{if(e.key==='Enter'&&(e.ctrlKey||e.metaKey)){e.preventDefault();void send();}}}></textarea>
 {#if slashOptions.length}<div class="slash-options" use:scrollbars aria-label="슬래시 명령">{#each slashOptions as c}<button type="button" onclick={()=>{text='/'+c.name+' ';save();}}><strong>/{c.name}</strong><small>{c.description}</small></button>{/each}</div>{/if}
 <div class="composer-options">
 <label><span class="sr-only">전송 방식</span><select bind:value={mode} disabled={pending!==null||sending}>
 {#if run?.capabilities.continue_session?.supported}<option value="continue">대화 이어가기</option>{/if}<option value="fresh">새 세션</option>
 {#if run?.capabilities.send_to_active.supported&&run.state==='running'}<option value="steer">현재 작업에 전달</option>{/if}</select></label>
 <label><span class="sr-only">모델 제공자</span><select bind:value={providerId} onchange={providerChanged} disabled={pending!==null||sending||(mode!=='fresh'&&!!run?.provider_id&&available.some(p=>p.id===run.provider_id))}><option value="" disabled>제공자 선택</option>{#each available as p}<option value={p.id}>{p.name}{p.host_id!=='local'?' · '+(hosts.find(h=>h.id===p.host_id)?.name??p.host_id):''}</option>{/each}</select></label>
 <input aria-label="모델" class="model-input" list={'models-'+(run?.id??'new')} placeholder="모델 이름" bind:value={model} onchange={rememberModel} disabled={pending!==null||sending||mode!=='fresh'} required={provider?.adapter==='ollama'||provider?.adapter==='open_ai'} />
 <datalist id={'models-'+(run?.id??'new')}>{#each suggestions as value}<option value={value}></option>{/each}</datalist>
 <button type="button" aria-label="제공자 설정" onclick={onsettings}>⚙</button>
 <button class="primary send" type="submit" disabled={sending||(!text.trim()&&!pending)||(mode==='continue'&&busySession&&!pending&&!/^\/(new|clear|rename|usage)(\s|$)/.test(text.trim()))}>{sending?'전송 중…':pending?'접수 확인·재시도':'전송'}</button>
 </div>
 {#if recent.length}<div class="model-history"><small>최근</small>{#each recent as h}<button type="button" disabled={sending||pending!==null||mode!=='fresh'} title={h.uses+'회 사용'} onclick={()=>{model=h.model;void rememberModel();}}>{h.model}</button>{/each}</div>{/if}
 {#if mode==='fresh'}<details class="execution-options"><summary>실행 옵션</summary><div class="form-grid"><label>역할<input bind:value={role} disabled={pending!==null||sending} /></label><label>호스트<select bind:value={host} onchange={()=>{if(provider?.host_id!==host){providerId='';model='';}}} disabled={pending!==null||sending}>{#each hosts as h}<option value={h.id}>{h.name}</option>{/each}</select></label><label class="check"><input type="checkbox" bind:checked={readOnly} disabled={pending!==null||sending} />읽기 전용</label></div></details>{/if}
 {#if mode==='continue'&&busySession}<small>현재 응답이 끝나면 이어서 보낼 수 있습니다.</small>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
</form>
