<script lang="ts">
import type {Project,Run,Work,Host,Provider,Submission,Receipt} from '../types';
import {command,request,ApiError} from '../api';
import {draftKey,loadDraft,saveDraft,submissionId} from '../drafts';
import {providers,shortId,isActive} from '../format';
import {sessionId} from '../sessions';
let {serverId,project,run=null,work=null,hosts,onaccepted}: {serverId:string;project:Project;run?:Run|null;work?:Work|null;hosts:Host[];onaccepted:(receipt:Receipt)=>void} = $props();
let text=$state(''),pending=$state<Submission|null>(null),sending=$state(false),error=$state(''),mode=$state<'continue'|'fresh'|'steer'>('fresh');
let provider=$state<Provider>('codex'),model=$state(''),role=$state('업무 조정'),host=$state('local'),readOnly=$state(false);
let loadedKey='';
const busySession=$derived(!!run&&(isActive(run.state)||['queued','uncertain','disconnected'].includes(run.state)));
const key=$derived(draftKey(serverId,project.id,work?.id??'new',sessionId(run??undefined)||'new'));
$effect(()=>{
 if(key===loadedKey)return;
 loadedKey=key;const saved=loadDraft(key);text=saved.text;pending=saved.pending;error='';mode=run?.capabilities.continue_session?.supported?'continue':'fresh';
 provider=run?.provider??'codex';model=run?.model??'';role=run?.role??'업무 조정';host=run?.host_id??hosts[0]?.id??'local';readOnly=run?.read_only??false;
});
function save(){try{saveDraft(key,{text,pending});}catch{error='이 브라우저에 초안을 저장할 수 없습니다.';}}
async function send(){
 if(sending||(!text.trim()&&!pending))return;
 const capturedKey=key;
 error='';sending=true;
 try{
 let payload:Submission=pending??{
  submission_id:submissionId(),project_key:project.id,work_id:work?.id??null,title:null,question:text,
  provider:mode!=='fresh'&&run?run.provider:provider,model:mode!=='fresh'&&run?run.model:model,host_id:mode!=='fresh'&&run?run.host_id:host,role:mode!=='fresh'&&run?run.role:role,mode,
  target_run_id:run?.id??null,expected_turn_id:run?.turn_id??null,expected_context_revision:work?.context_revision??null,read_only:mode!=='fresh'&&run?run.read_only:readOnly||provider==='ollama'
 };
  let receipt:Receipt|undefined;
  if(pending){
   try{receipt=await request<Receipt>('/api/submissions/'+pending.submission_id);}
   catch(e){if(!(e instanceof ApiError)||e.status!==404)throw e;}
  }
  pending=payload;saveDraft(capturedKey,{text:payload.question,pending:payload});
  receipt??=await command<Receipt>({type:'submit',request:payload});
  saveDraft(capturedKey,{text:'',pending:null});
  if(key===capturedKey){text='';pending=null;}
  onaccepted(receipt);
 }catch(e){
  if(key===capturedKey){
   error=e instanceof Error?e.message:String(e);
   if(e instanceof ApiError && e.status>=400 && e.status<500 && e.status!==408){pending=null;save();}
   else error+=' · 접수 확인 후 같은 전송을 재시도합니다.';
  }
 }finally{sending=false;}
}
</script>
<form class="composer" onsubmit={(e)=>{e.preventDefault();void send();}}>
 <div class="compose-target">{#if run}세션 {shortId(sessionId(run))} · {providers[run.provider]} · {run.host_id}{:else}{project.name} · 새 업무{/if}</div>
 <label class="sr-only" for={'message-'+(run?.id??'new')}>메시지</label>
 <textarea id={'message-'+(run?.id??'new')} bind:value={text} oninput={save} disabled={sending||pending!==null} rows="3" placeholder="메시지 입력"
  onkeydown={(e)=>{if(e.key==='Enter'&&(e.ctrlKey||e.metaKey)){e.preventDefault();void send();}}}></textarea>
 <div class="composer-options">
  <label><span class="sr-only">전송 방식</span><select bind:value={mode} disabled={pending!==null||sending}>
   {#if run?.capabilities.continue_session?.supported}<option value="continue">대화 이어가기</option>{/if}
   <option value="fresh">새 세션</option>
   {#if run?.capabilities.send_to_active.supported&&run.state==='running'}<option value="steer">현재 작업에 전달</option>{/if}
  </select></label>
  {#if mode==='fresh'}
   <label><span class="sr-only">서비스</span><select bind:value={provider} disabled={pending!==null||sending}>
    <option value="codex">Codex</option><option value="claude">Claude Code</option><option value="ollama">Ollama</option><option value="mock">모의 실행</option>
   </select></label>
   <input aria-label="모델" class="model-input" placeholder={provider==='ollama'?'모델 이름':'기본 모델'} bind:value={model} disabled={pending!==null||sending} required={provider==='ollama'} />
  {/if}
  {#if provider==='ollama'}<small>프로젝트 파일 읽기 · 길드 기록</small>{/if}
  <button class="primary send" type="submit" disabled={sending||(!text.trim()&&!pending)||(mode==='continue'&&busySession&&!pending)}>{sending?'전송 중…':pending?'접수 확인·재시도':'전송'}</button>
 </div>
 {#if mode==='fresh'}<details class="execution-options"><summary>실행 옵션</summary><div class="form-grid">
  <label>역할<input bind:value={role} disabled={pending!==null||sending} /></label>
  <label>호스트<select bind:value={host} disabled={pending!==null||sending}>{#each hosts as h}<option value={h.id}>{h.name}</option>{/each}</select></label>
  <label class="check"><input type="checkbox" bind:checked={readOnly} disabled={pending!==null||sending} />읽기 전용</label>
 </div></details>{/if}
 {#if mode==='continue'&&busySession}<small>현재 응답이 끝나면 이어서 보낼 수 있습니다.</small>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
</form>
