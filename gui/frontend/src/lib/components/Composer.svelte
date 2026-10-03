<script lang="ts">
import {reveal,disclosure} from '../motion';
import {untrack} from 'svelte';
import Icon from './Icon.svelte';
import {scrollbars} from '../scrollbars';
import type {ApprovalMode,Project,Run,Work,Host,Submission,Receipt,ProviderConfig,ModelHistory,ModelSelection} from '../types';
import {command,request,ApiError} from '../api';
import {draftKey,loadDraft,saveDraft,submissionId} from '../drafts';
import {isActive} from '../format';
import {modelSuggestions,recentModels} from '../models';
import {sessionId} from '../sessions';
import {approvalModes,approvalLabel,approvalDescription} from '../permissions';
let {serverApprovals=false,serverId,project,run=null,work=null,hosts,providers,modelHistory,selection,onaccepted,onsettings,onlocal}: {serverApprovals?:boolean;serverId:string;project:Project;run?:Run|null;work?:Work|null;hosts:Host[];providers:ProviderConfig[];modelHistory:ModelHistory[];selection:ModelSelection|null;onaccepted:(receipt:Receipt)=>void;onsettings:()=>void;onlocal:(name:string,arg:string)=>Promise<void>} = $props();
let text=$state(''),pending=$state<Submission|null>(null),sending=$state(false),error=$state(''),mode=$state<'continue'|'fresh'|'steer'>('fresh');
let providerId=$state(''),model=$state(''),role=$state('업무 조정'),host=$state('local'),readOnly=$state(false);
let approvalMode=$state<ApprovalMode>('on_request');
let loadedKey='',syncedRun='',syncedModel='',syncedApproval:ApprovalMode='on_request';
let historyOpen=$state(false),settingsOpen=$state(false);
let input=$state<HTMLTextAreaElement>(),modelInput=$state<HTMLInputElement>(),approvalSelect=$state<HTMLSelectElement>();
type MenuCommand={name:string;description:string;argument_hint?:string;source:string;supported:boolean;reason?:string};
let catalog=$state<MenuCommand[]>([]),catalogError=$state(''),catalogLoading=$state(false);
let catalogKey='',catalogRequest=0;
export function focus(){input?.focus({preventScroll:true});}
const busySession=$derived(!!run&&(isActive(run.state)||['queued','uncertain','disconnected'].includes(run.state)));
const localCommand=$derived(/^\/bibi\s+(new|rename|usage|extensions|model|permissions)(\s|$)/.test(text.trim()));
const canSend=$derived(!sending&&(!!text.trim()||pending!==null)&&!(mode==='continue'&&busySession&&!pending&&!localCommand));
const key=$derived(draftKey(serverId,project.id,work?.id??'new',sessionId(run??undefined)||'new'));
const available=$derived(mode==='fresh'?providers:providers.filter(p=>p.adapter===run?.provider&&p.host_id===run?.host_id));
const provider=$derived(providers.find(p=>p.id===providerId));
const approvalProvider=$derived(mode!=='fresh'&&run?run.provider:provider?.adapter);
const approvalOptions=$derived(approvalModes(approvalProvider));
const approvalReadOnly=$derived(mode!=='fresh'&&run?run.read_only:readOnly);
const suggestions=$derived(modelSuggestions(provider,modelHistory));
const recent=$derived(recentModels(providerId,modelHistory));
const inputId=$derived('message-'+(run?.id??'new'));
const waitingDescription=$derived(mode==='continue'&&busySession&&!localCommand&&!pending&&!sending
 ?run?.state==='disconnected'||run?.state==='uncertain'?'실행 상태를 확인한 뒤 이어서 보낼 수 있습니다.'
 :run?.state==='waiting_user'?'승인 또는 입력 요청을 처리한 뒤 이어서 보낼 수 있습니다.'
 :'현재 응답이 끝나면 이어서 보낼 수 있습니다.':'');
const approvalHelp=$derived(!serverApprovals?'연결된 BiBi 서버를 업데이트해야 승인 모드를 변경할 수 있습니다.'
 :approvalReadOnly?'읽기 전용 세션에서는 승인 범위를 변경할 수 없습니다.'
 :mode==='steer'?'현재 작업에 전달할 때는 진행 중인 턴의 승인 모드를 유지합니다.'
 :(mode==='fresh'?'새 세션에 적용':approvalMode!==(run?.approval_mode??'on_request')?'다음 메시지부터 적용':'현재 승인 모드')+' · '+approvalDescription(approvalMode,approvalProvider));
const slash=$derived(text.startsWith('/')&&!text.startsWith('//')&&text.length<128?text.slice(1).trimEnd():null);
const appCommands:MenuCommand[]=[['new','새 세션'],['rename','세션 이름 변경'],['usage','사용량'],['extensions','프로젝트 확장 설정'],['model','다음 메시지의 모델'],['permissions','다음 메시지의 승인 모드']].map(([name,description])=>({name:'bibi '+name,description,source:'bibi',supported:true}));
const slashOptions:MenuCommand[]=$derived((catalog.length?catalog:[...appCommands,...(run?.runtime?.commands??[]).map(c=>({...c,source:'provider',supported:true}))]).filter(c=>slash!==null&&c.name.startsWith(slash)));
$effect(()=>{
 if(slash===null||slash.startsWith('bibi ')||!providerId)return;
 const next=project.id+':'+providerId+':'+(run?.id??'new');
 if(next===catalogKey)return;catalogKey=next;const id=++catalogRequest;catalog=[];catalogError='';catalogLoading=true;
 command<{entries:MenuCommand[];errors:string[]}>({type:'list_commands',project_key:project.id,provider_id:providerId,run_id:run?.id??null}).then(value=>{if(id===catalogRequest){catalog=value.entries;catalogError=value.errors.join(' ');}}).catch(e=>{if(id===catalogRequest)catalogError=e instanceof Error?e.message:String(e);}).finally(()=>{if(id===catalogRequest)catalogLoading=false;});
});
$effect(()=>{
 if(key===loadedKey)return;
 loadedKey=key;historyOpen=false;settingsOpen=!run;const saved=loadDraft(key);text=saved.text;pending=saved.pending;error='';mode=run?.capabilities.continue_session?.supported?'continue':'fresh';
 providerId=run?.provider_id??(providers.some(p=>p.id===selection?.provider_id&&(!run||p.adapter===run.provider))?selection!.provider_id:'');model=run?.model??selection?.model??'';role=run?.role??'업무 조정';host=run?.host_id??providers.find(p=>p.id===providerId)?.host_id??'local';readOnly=pending?.read_only??run?.read_only??false;
 const savedApproval=saved.approval?.provider===providerId&&saved.approval.run===(run?.id??'new')?saved.approval.mode:undefined;
 approvalMode=pending?.approval_mode??savedApproval??(mode!=='fresh'?run?.approval_mode:undefined)??'on_request';
 if(!serverApprovals||!approvalModes(provider?.adapter??run?.provider).includes(approvalMode)||readOnly)approvalMode='on_request';
 syncedApproval=run?.approval_mode??'on_request';
});
$effect(()=>{
 if(mode==='fresh'||!run)return;
 const id=run.id,actual=run.model,actualApproval=run.approval_mode??'on_request';
 untrack(()=>{if(mode==='steer'||id!==syncedRun||model===syncedModel)model=actual;
 if(mode==='steer'||(syncedRun!==''&&id!==syncedRun)||approvalMode===syncedApproval)approvalMode=actualApproval;});
 syncedApproval=actualApproval;
 syncedRun=id;syncedModel=actual;
 if(run.provider_id)providerId=run.provider_id;
});
function modeChanged(){if(mode!=='fresh'&&run){model=run.model;providerId=run.provider_id??providerId;}approvalMode=mode==='fresh'?'on_request':run?.approval_mode??'on_request';historyOpen=false;save();}
function toggleHistory(){historyOpen=!historyOpen;if(historyOpen)settingsOpen=false;}
function toggleSettings(){settingsOpen=!settingsOpen;if(settingsOpen)historyOpen=false;}
function approvalChanged(){save();}
function readOnlyChanged(){if(readOnly)approvalMode='on_request';save();}

function save(){try{saveDraft(key,{text,pending,approval:{mode:approvalMode,provider:providerId,run:run?.id??'new'}});}catch{error='이 브라우저에 초안을 저장할 수 없습니다.';}}
async function rememberModel(){if(!provider)return;try{await command({type:'select_model',selection:{provider_id:providerId,model:model.trim()}});}catch(e){error=e instanceof Error?e.message:String(e);}}
function providerChanged(){approvalMode=mode!=='fresh'?run?.approval_mode??'on_request':'on_request';save();host=mode!=='fresh'&&run?run.host_id:provider?.host_id??'local';model=mode!=='fresh'&&run?run.model:recentModels(providerId,modelHistory)[0]?.model??'';void rememberModel();}
async function send(){
 if(!canSend)return;
 // Focus synchronously with the submit gesture, before any network work.
 focus();
 const capturedKey=key;error='';sending=true;
 try{
 const match=!pending&&text.trim().match(/^\/bibi\s+(new|rename|usage|extensions|model|permissions)(?:\s+([\s\S]*))?$/);
 if(match){
  const arg=match[2]??'';
  if(match[1]==='model'){if(arg){model=arg.trim();await rememberModel();}else modelInput?.focus();}
  else if(match[1]==='permissions'){if(arg){if(!approvalOptions.includes(arg as ApprovalMode)||approvalReadOnly)throw new Error('이 세션에서 선택할 수 없는 승인 모드입니다.');approvalMode=arg as ApprovalMode;}else approvalSelect?.focus();}
  else await onlocal(match[1],arg);
  text='';save();return;
 }
 const slashMatch=text.trim().match(/^\/([\w:.@-]+)(?:\s|$)/);
 if(!pending&&slashMatch&&mode==='steer')throw new Error('명령은 현재 응답이 끝난 뒤 실행하세요.');
 if(!pending&&slashMatch&&catalog.length){const selected=catalog.find(c=>c.name===slashMatch[1]);if(!selected||!selected.supported)throw new Error(selected?.reason||'지원하지 않는 명령입니다. 문자 그대로 보내려면 //로 시작하세요.');}
 if(!pending&&!provider)throw new Error('제공자를 등록하고 선택하세요.');
 let payload:Submission=pending??{
  submission_id:submissionId(),project_key:project.id,work_id:work?.id??null,title:null,question:text,
  provider:provider!.adapter,provider_id:providerId,model:mode!=='fresh'&&run?(mode==='steer'?run.model:model.trim()||run.model):model.trim(),host_id:mode!=='fresh'&&run?run.host_id:host,role:mode!=='fresh'&&run?run.role:role,mode,
  target_run_id:run?.id??null,expected_turn_id:run?.turn_id??null,expected_context_revision:work?.context_revision??null,read_only:mode!=='fresh'&&run?run.read_only:readOnly||provider?.adapter==='ollama',
  ...(approvalOptions.length?{approval_mode:approvalReadOnly?'on_request':mode==='steer'?run?.approval_mode??'on_request':approvalMode}:{} )
 };
 if(payload.approval_mode&&payload.approval_mode!=='on_request'&&!serverApprovals)throw new Error('연결된 BiBi 서버를 업데이트해야 승인 모드를 변경할 수 있습니다.');
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
 <label class="sr-only" for={inputId}>메시지</label>
 <textarea bind:this={input} use:scrollbars id={inputId} aria-describedby={waitingDescription?inputId+'-waiting':undefined} bind:value={text} oninput={save} readonly={sending||pending!==null} rows="2" placeholder="메시지 입력 · / 명령"
 onkeydown={(e)=>{if(e.key==='Enter'&&(e.ctrlKey||e.metaKey)){e.preventDefault();void send();}}}></textarea>
 {#if slashOptions.length}<div transition:reveal class="slash-options" use:scrollbars aria-label="슬래시 명령">{#each slashOptions as c}<button type="button" disabled={!c.supported} title={c.reason||undefined} onclick={()=>{text='/'+c.name+' ';save();focus();}}><strong>/{c.name}</strong><small>{c.description}{c.argument_hint?' · '+c.argument_hint:''}</small><small>{c.source==='bibi'?'BiBi':c.source==='skill'?'스킬':'제공자'}{!c.supported?' · '+c.reason:''}</small></button>{/each}</div>{/if}
 {#if slash!==null&&catalogLoading}<span class="sr-only" role="status">명령 조회 중</span>{/if}
 {#if slash!==null&&catalogError}<p class="command-hint" role="status">{catalogError}</p>{/if}
 <div class="composer-options" class:has-approval={approvalOptions.length>0}>
 <div class="composer-model">
 <input bind:this={modelInput} aria-label="모델" title={model||'다음 메시지의 모델'} class="model-input" list={'models-'+(run?.id??'new')} placeholder="모델 이름" bind:value={model} onchange={rememberModel} disabled={pending!==null||sending||mode==='steer'} required={provider?.adapter==='ollama'||provider?.adapter==='open_ai'} />
 <datalist id={'models-'+(run?.id??'new')}>{#each suggestions as value}<option value={value}></option>{/each}</datalist>
 {#if recent.length}<button class="icon-button" type="button" aria-label="최근 모델" title="최근 모델" aria-expanded={historyOpen} onclick={toggleHistory}><Icon name="history" /></button>{/if}
 </div>
 <div class="composer-actions">
 {#if approvalOptions.length}
 <select bind:this={approvalSelect} class="approval-mode" aria-label="승인 모드" title={approvalHelp} aria-describedby={inputId+'-approval'} bind:value={approvalMode} onchange={approvalChanged} disabled={!serverApprovals||pending!==null||sending||mode==='steer'||approvalReadOnly}>
 {#if approvalReadOnly}<option value="on_request">읽기 전용</option>{:else}{#each approvalOptions as value}<option {value}>{approvalLabel(value,approvalProvider)}</option>{/each}{/if}
 </select>
 <span class="sr-only" id={inputId+'-approval'} role="status">{approvalHelp}</span>
 {/if}
 <button class="icon-button" type="button" aria-label="전송 설정" title={mode==='fresh'?'새 세션 설정':mode==='steer'?'현재 작업에 전달':'대화 이어가기 설정'} aria-expanded={settingsOpen} onclick={toggleSettings}><Icon name="settings" /></button>
 <button class="primary send" type="submit" title={waitingDescription||undefined} aria-describedby={waitingDescription?inputId+'-waiting':undefined} disabled={!canSend}>{sending?'전송 중…':pending?'접수 확인·재시도':'전송'}</button>
 </div>
 </div>
 {#if historyOpen&&recent.length}<div transition:reveal class="model-history"><small>최근</small>{#each recent as h}<button type="button" disabled={sending||pending!==null||mode==='steer'} title={h.uses+'회 사용'} onclick={()=>{model=h.model;historyOpen=false;void rememberModel();focus();}}>{h.model}</button>{/each}</div>{/if}
 {#if settingsOpen}<div transition:reveal class="composer-settings">
 <label><span class="sr-only">전송 방식</span><select bind:value={mode} onchange={modeChanged} disabled={pending!==null||sending}>
 {#if run?.capabilities.continue_session?.supported}<option value="continue">대화 이어가기</option>{/if}<option value="fresh">새 세션</option>
 {#if run?.capabilities.send_to_active.supported&&run.state==='running'}<option value="steer">현재 작업에 전달</option>{/if}</select></label>
 <label><span class="sr-only">모델 제공자</span><select bind:value={providerId} onchange={providerChanged} disabled={pending!==null||sending||(mode!=='fresh'&&!!run?.provider_id&&available.some(p=>p.id===run.provider_id))}><option value="" disabled>제공자 선택</option>{#each available as p}<option value={p.id}>{p.name}{p.host_id!=='local'?' · '+(hosts.find(h=>h.id===p.host_id)?.name??p.host_id):''}</option>{/each}</select></label>
 <button class="icon-button" type="button" aria-label="제공자 설정" title="제공자 설정" onclick={onsettings}><Icon name="plus" /></button>
 {#if mode==='fresh'}<details use:disclosure class="execution-options"><summary>실행 옵션</summary><div class="form-grid"><label>역할<input bind:value={role} disabled={pending!==null||sending} /></label><label>호스트<select bind:value={host} onchange={()=>{if(provider?.host_id!==host){providerId='';model='';}}} disabled={pending!==null||sending}>{#each hosts as h}<option value={h.id}>{h.name}</option>{/each}</select></label><label class="check"><input type="checkbox" bind:checked={readOnly} onchange={readOnlyChanged} disabled={pending!==null||sending} />읽기 전용</label></div></details>{/if}
 </div>{/if}
 {#if waitingDescription}<span class="sr-only" id={inputId+'-waiting'} aria-live="polite">{waitingDescription}</span>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
</form>
