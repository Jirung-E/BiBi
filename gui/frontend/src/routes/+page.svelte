<script lang="ts">
import {onMount,untrack} from 'svelte';
import {modalDialog} from '$lib/modal';
import {pushState,replaceState} from '$app/navigation';
import {page} from '$app/state';
import {readNavigation,navigationUrl,type Navigation} from '$lib/navigation';
import {ApiError,command,request,login,subscribe,connection,setConnection,isDesktop} from '$lib/api';
import type {Snapshot,Detail,Run,Project,Work,Event,Receipt,Quota,Approval,ProviderConfig,ModelHistory,ModelSelection} from '$lib/types';
import {product,providers,providerName,states,shortId,age,dateTime,isActive} from '$lib/format';
import {sessionId,sessionNodes,sessionEdges} from '$lib/sessions';
import Canvas from '$lib/components/Canvas.svelte';
import Icon from '$lib/components/Icon.svelte';
import QuotaCards from '$lib/components/QuotaCards.svelte';
import Composer from '$lib/components/Composer.svelte';
import RunDetails from '$lib/components/RunDetails.svelte';
import ProviderSettings from '$lib/components/ProviderSettings.svelte';
import Markdown from '$lib/components/Markdown.svelte';
import SessionActions from '$lib/components/SessionActions.svelte';
import ApprovalForm from '$lib/components/ApprovalForm.svelte';
let snapshot=$state<Snapshot|null>(null),detail=$state<Detail|null>(null);
let messagesPane=$state<HTMLDivElement>();
let followTail=$state(true);
$effect(()=>{const last=(detail?.conversation??detail?.messages)?.at(-1);const revision=last?.id+':'+last?.text;if(revision&&followTail&&messagesPane)requestAnimationFrame(()=>messagesPane?.scrollTo({top:messagesPane.scrollHeight}));});
let selected=$state(''),projectId=$state(''),view=$state<'canvas'|'conversation'|'usage'>('canvas');
let connected=$state(false),loading=$state(true),needsAuth=$state(false),error=$state(''),token=$state(''),now=$state(Date.now());
let modal=$state<''|'project'|'new'|'settings'|'context'|'host'>('');
let name=$state(''),workspace=$state(''),guild=$state(''),remoteUrl=$state(''),remoteToken=$state(''),endpoint=$state('');
let hostName=$state(''),hostUrl=$state(''),hostToken=$state(''),hostWorkspace=$state(''),hostGuild=$state(''),savingHost=$state(false);
let uiScale=$state(1),routeReady=$state(false);
let windowWidth=$state(0),sidebarOpen=$state(true),sidebarDrawer=$state(false),contextOpen=$state(false);
const compactNavigation=$derived(windowWidth<1000*uiScale);
function toggleSidebar(){
 if(compactNavigation)sidebarDrawer=!sidebarDrawer;
 else {sidebarOpen=!sidebarOpen;try{localStorage.setItem('bibi:sidebar',JSON.stringify(sidebarOpen));}catch{/* Optional preference. */}}
}
$effect(()=>{if(!compactNavigation)sidebarDrawer=false;});
let halfLife=$state(30),floor=$state(.15),contextGoal=$state(''),contextConstraints=$state('');
let unsubscribe=()=>{},detailTimer:ReturnType<typeof setTimeout>|undefined,detailGeneration=0,detailDirty=0;
const project=$derived(snapshot?.projects.find(p=>p.id===projectId)??snapshot?.projects[0]);
const runs=$derived(snapshot?.runs.filter(r=>!project||r.project_key===project.id)??[]);
const works=$derived(snapshot?.works.filter(w=>!project||w.project_key===project.id)??[]);
const run=$derived(runs.find(r=>r.id===selected));
const work=$derived(works.find(w=>w.id===run?.work_id));
const edges=$derived(snapshot?.transmissions.filter(t=>runs.some(r=>r.id===t.to_run_id))??[]);
const quotas=$derived.by(()=>{
 const values=(snapshot?.quotas??[]).filter(q=>snapshot?.providers.some(p=>p.id===q.provider_id));
 return [...values,...(snapshot?.providers??[]).filter(p=>!values.some(q=>q.provider_id===p.id)).map(p=>({id:'pending:'+p.id,provider:p.adapter,provider_id:p.id,account:p.name,host_id:p.host_id,model:null,status:'unknown',windows:[],observed_at:null,reason:'사용량 갱신 대기'} as Quota))];
});
const discoverProviders=$derived(snapshot?.providers.filter(p=>p.adapter==='codex'&&p.host_id==='local')??[]);
const sessions=$derived(sessionNodes(runs));
const canvasEdges=$derived(sessionEdges(runs,edges));
const sessionHistory=$derived(sessions.filter(r=>r.work_id===run?.work_id));
onMount(()=>{
 try{const s=JSON.parse(localStorage.getItem('bibi:appearance')??'null');if(s){halfLife=s.halfLife??30;floor=s.floor??.15;uiScale=Math.max(.5,Math.min(2,s.uiScale??1));}}catch{/* Defaults. */}
 document.documentElement.style.fontSize=14*uiScale+'px';
 try{sidebarOpen=JSON.parse(localStorage.getItem('bibi:sidebar')??'true')!==false;}catch{/* Default expanded. */}
 void connect();const timer=setInterval(()=>{now=Date.now();},1000);
 return()=>{unsubscribe();clearInterval(timer);clearTimeout(detailTimer);};
});
function currentNavigation():Navigation{return {view,project:projectId,run:selected,modal};}
function applyNavigation(next:Navigation){
 const changed=selected!==next.run;view=next.view;projectId=next.project;selected=next.run;modal=next.modal;
 if(changed){detail=null;followTail=true;if(selected)void loadDetail(selected);}
 if(modal==='context'&&work){contextGoal=work.goal;contextConstraints=work.constraints.join('\n');}remember();
}
function navigate(change:Partial<Navigation>,replace=false){
 sidebarDrawer=false;
 const next={...currentNavigation(),...change};const current=new URL(window.location.href);const url=navigationUrl(current,next);
 if(url.href!==current.href)(replace?replaceState:pushState)(url,{bibi:next,bibiModal:!!next.modal&&(!replace||!!(page.state as {bibiModal?:boolean}).bibiModal)});
 applyNavigation(next);
}
$effect(()=>{const next=(page.state as {bibi?:Navigation}).bibi;if(routeReady&&next)untrack(()=>applyNavigation(next));});
function showModal(next:Navigation['modal']){navigate({modal:next});}
function closeModal(){if(!modal)return;if((page.state as {bibiModal?:boolean}).bibiModal)window.history.back();else navigate({modal:''},true);}
function restoreSelection(){
 if(!snapshot)return;
 const url=new URL(window.location.href);let next=readNavigation(url);
 if(!url.searchParams.has('view')){try{const saved=JSON.parse(localStorage.getItem('bibi:selection:'+snapshot.server_id)??'null');if(saved){next.project=saved.project??'';next.run=saved.run??'';}}catch{/* Empty selection. */}}
 if(!snapshot.projects.some(p=>p.id===next.project))next.project=snapshot.projects[0]?.id??'';
 if(!snapshot.runs.some(r=>r.id===next.run&&r.project_key===next.project))next.run='';
 applyNavigation(next);replaceState(navigationUrl(url,next),{bibi:next});routeReady=true;
}
function appearance(){uiScale=Math.max(.5,Math.min(2,uiScale||1));requestAnimationFrame(()=>document.documentElement.style.fontSize=14*uiScale+'px');localStorage.setItem('bibi:appearance',JSON.stringify({halfLife,floor,uiScale}));}
async function refreshSnapshot(){snapshot=await request<Snapshot>('/api/snapshot');if(selected&&!snapshot.runs.some(r=>r.id===selected))navigate({run:''},true);if(selected)await loadDetail(selected);}
async function localCommand(name:string,arg:string){
 if(name==='new'||name==='clear'){showModal(project?'new':'project');return;}
 if(name==='usage'){navigate({view:'usage',modal:''});await action({type:'refresh_providers'});return;}
 if(name==='rename'){if(!run||!arg.trim())throw new Error('/rename 새 이름 형식으로 입력하세요.');await command({type:'rename_session',run_id:run.id,title:arg});await refreshSnapshot();}
}
function remember(){if(snapshot)localStorage.setItem('bibi:selection:'+snapshot.server_id,JSON.stringify({project:project?.id,run:selected}));}
async function connect(){
 loading=true;error='';unsubscribe();
 try{
  const info=await connection();endpoint=info.url;
  snapshot=await request<Snapshot>('/api/snapshot');needsAuth=false;connected=true;restoreSelection();
  if(!snapshot.projects.some(p=>p.id===projectId))projectId=snapshot.projects[0]?.id??'';
  if(!snapshot.runs.some(r=>r.id===selected&&r.project_key===projectId))selected='';
  unsubscribe=subscribe(snapshot.last_seq,update,v=>connected=v);
  if(selected)void loadDetail(selected);
 }catch(e){needsAuth=e instanceof ApiError&&e.status===401;error=String(e instanceof Error?e.message:e);connected=false;}
 finally{loading=false;}
}
async function authenticate(){error='';try{await login(token);token='';await connect();}catch(e){error=String(e instanceof Error?e.message:e);}}
function upsert<T extends {id:string}>(list:T[],value:T){const index=list.findIndex(item=>item.id===value.id);if(index<0)list.push(value);else list[index]=value;}
function update(event:Event){
 if(!snapshot||event.seq<=snapshot.last_seq)return;
 snapshot.last_seq=event.seq;
 switch(event.kind){
  case 'run':{const next=event.data as Run;const current=snapshot.runs.find(r=>r.id===selected);upsert(snapshot.removed_sessions.some(r=>sessionId(r)===sessionId(next))?snapshot.removed_sessions:snapshot.runs,next);if(current&&next.continued_from===selected&&sessionId(next)===sessionId(current)){navigate({run:next.id},true);}break;}
  case 'provider':upsert(snapshot.providers,event.data as ProviderConfig);break;
  case 'provider_deleted':{const id=(event.data as {id:string}).id;snapshot.providers=snapshot.providers.filter(p=>p.id!==id);snapshot.quotas=snapshot.quotas.filter(q=>q.provider_id!==id);if(snapshot.model_selection?.provider_id===id)snapshot.model_selection=null;break;}
  case 'model_selection':snapshot.model_selection=event.data as ModelSelection;break;
  case 'model_history':{const item=event.data as ModelHistory;snapshot.model_history=[...snapshot.model_history.filter(h=>h.provider_id!==item.provider_id||h.model!==item.model),item];break;}
  case 'session_visibility':void refreshSnapshot();break;
  case 'work':upsert(snapshot.works,event.data as Work);break;
  case 'project':upsert(snapshot.projects,event.data as Project);break;
  case 'host':upsert(snapshot.hosts,event.data as Snapshot['hosts'][number]);break;
  case 'quota':upsert(snapshot.quotas,event.data as Quota);break;
  case 'quotas':{const q=event.data as {provider:string;provider_id?:string;host_id:string;quotas:Quota[]};snapshot.quotas=[...snapshot.quotas.filter(v=>(q.provider_id?v.provider_id!==q.provider_id:v.provider!==q.provider)||v.host_id!==q.host_id),...q.quotas];break;}
  case 'transmission':upsert(snapshot.transmissions,event.data as Snapshot['transmissions'][number]);break;
  case 'approval':upsert(snapshot.approvals,event.data as Approval);break;
  case 'inbox':{const entry=event.data as Snapshot['inbox'][number];if(!snapshot.inbox.some(e=>e.response_id===entry.response_id))snapshot.inbox.push(entry);break;}
 }
 const data=event.data as {run_id?:string;id?:string;from_run_id?:string};
 if(selected&&(data.run_id===selected||(event.kind==='run'&&data.id===selected)||data.from_run_id===selected)){
  detailDirty++;
  if(!detailTimer)detailTimer=setTimeout(()=>{detailTimer=undefined;void loadDetail(selected);},150);
 }
}
async function loadDetail(id:string){
 const generation=++detailGeneration,dirty=detailDirty;
 try{
  const value=await request<Detail>('/api/runs/'+id);
  if(selected===id&&generation===detailGeneration){detail=value;if(detailDirty!==dirty&&!detailTimer)detailTimer=setTimeout(()=>{detailTimer=undefined;void loadDetail(id);},150);}
 }catch(e){if(selected===id)error=String(e instanceof Error?e.message:e);}
}
function select(id:string){navigate({run:id});}
function open(id:string){navigate({run:id,view:'conversation'});}
async function accepted(receipt:Receipt){
 const previous=run;snapshot=await request<Snapshot>('/api/snapshot');const next=snapshot.runs.find(r=>r.id===receipt.run_id);navigate({run:receipt.run_id,view:'conversation',modal:''},!!modal||sessionId(previous)===sessionId(next));
}
async function createProject(){
 error='';try{
  const p=await command<Project>({type:'create_project',name,workspace,guild_path:guild||null,constraints:[]});
  if(snapshot)upsert(snapshot.projects,p);navigate({project:p.id,run:'',modal:'new'},true);name='';workspace='';guild='';remember();
 }catch(e){error=String(e instanceof Error?e.message:e);}
}
async function action(body:unknown){
 error='';try{await command(body);if(selected)await loadDetail(selected);}catch(e){error=String(e instanceof Error?e.message:e);}
}
function editContext(){if(!work)return;contextGoal=work.goal;contextConstraints=work.constraints.join('\n');showModal('context');}
async function saveContext(){
 if(!work)return;error='';
 try{const updated=await command<Work>({type:'update_context',work_id:work.id,update:{...work,expected_revision:work.context_revision,goal:contextGoal,constraints:contextConstraints.split('\n').filter(s=>s.trim())}});
  if(snapshot)upsert(snapshot.works,updated);closeModal();
 }catch(e){error=String(e instanceof Error?e.message:e);}
}
function projectChanged(){navigate({project:projectId,run:''});}
async function registerHost(){
 if(!project||savingHost)return;savingHost=true;error='';
 try{await command({type:'register_host',name:hostName,url:hostUrl,token:hostToken,project_key:project.id,workspace:hostWorkspace,guild_path:hostGuild||null});hostToken='';closeModal();snapshot=await request<Snapshot>('/api/snapshot');}
 catch(e){error=e instanceof Error?e.message:String(e);}finally{savingHost=false;}
}
async function changeConnection(){
 unsubscribe();routeReady=false;detailGeneration++;
 try{await setConnection(remoteUrl,remoteToken);remoteToken='';snapshot=null;selected='';detail=null;modal='';await connect();}catch(e){error=String(e);}
}
</script>

<svelte:head><title>{product.name}</title><meta name="description" content="BiBi 세션 캔버스" /></svelte:head>
<svelte:window bind:innerWidth={windowWidth} />

{#snippet sidebar()}
 <div class="sidebar-head"><button class="brand" onclick={()=>navigate({view:'canvas'})}>{product.name}</button><button class="icon-button" aria-label="사이드바 닫기" title="사이드바 닫기" onclick={()=>{if(compactNavigation)sidebarDrawer=false;else toggleSidebar();}}><Icon name="sidebar" /></button></div>
 {#if snapshot}
  <div class="sidebar-project"><span class="sidebar-label">프로젝트</span><div class="project-controls">
   {#if snapshot.projects.length}<div class="project-select"><select aria-label="프로젝트" class="project-picker" bind:value={projectId} onchange={projectChanged}>{#each snapshot.projects as p}<option value={p.id}>{p.name}</option>{/each}</select></div>{:else}<span class="muted">프로젝트 없음</span>{/if}
   <button class="icon-button project-add" aria-label="프로젝트 추가" title="프로젝트 추가" onclick={()=>showModal('project')}><Icon name="plus" /></button>
  </div></div>
  <nav class="navigation" aria-label="주요 메뉴">
   <button class:active={view==='canvas'} aria-current={view==='canvas'?'page':undefined} onclick={()=>navigate({view:'canvas'})}><Icon name="canvas" /><span>세션 캔버스</span><small aria-hidden="true">{sessions.length}</small></button>
   <button class:active={view==='conversation'} aria-current={view==='conversation'?'page':undefined} onclick={()=>navigate({view:'conversation'})}><Icon name="chat" /><span>작업 대화</span></button>
   <button class:active={view==='usage'} aria-current={view==='usage'?'page':undefined} onclick={()=>navigate({view:'usage'})}><Icon name="usage" /><span>사용량·연결</span></button>
  </nav>
  <div class="sidebar-scroll">
   {#if view==='conversation'&&run}<section class="sidebar-section history" aria-label="업무 세션"><h2 class="sidebar-label">세션</h2>{#each sessionHistory as item(item.id)}<button class:active={sessionId(item)===sessionId(run)} onclick={()=>select(item.id)}><span>{item.title}</span><small>{item.agent_kind==='subagent'?'서브에이전트':providerName(item,snapshot.providers)} · {states[item.state]}</small></button>{/each}</section>{/if}
   {#if quotas.length}<section class="sidebar-section sidebar-usage"><h2 class="sidebar-label">사용량</h2><QuotaCards {quotas} {now} connections={snapshot.providers} compact onopen={()=>navigate({view:'usage'})} /></section>{/if}
  </div>
 {/if}
 <div class="sidebar-footer">
  {#if snapshot}<div class="sidebar-connection"><span class={'connection-dot '+(connected?'connected':'warn')} aria-hidden="true"></span><small>{connected?'연결됨':'재연결 중'} · 호스트 {snapshot.hosts.length}</small></div>{/if}
  <button class="settings-button" onclick={()=>showModal('settings')}><Icon name="settings" /><span>설정</span></button>
 </div>
{/snippet}

<div class="app-shell" class:sidebar-expanded={!compactNavigation&&sidebarOpen} class:canvas-view={!!snapshot&&view==='canvas'}>
 <aside class="app-sidebar" aria-label="사이드바" inert={compactNavigation||!sidebarOpen}>{#if !compactNavigation}{@render sidebar()}{/if}</aside>
 <div class="workspace-shell">
 <header class="content-toolbar">
  <button class="icon-button sidebar-toggle" aria-label="사이드바 열기" aria-expanded={compactNavigation?sidebarDrawer:sidebarOpen} title="사이드바" onclick={toggleSidebar}><Icon name="sidebar" /></button>
  <div class="toolbar-title"><h1>{view==='canvas'?'세션 캔버스':view==='conversation'?(work?.title??'작업 대화'):'사용량·연결'}</h1><small title={project?.name}>{view==='canvas'?works.length+'개 업무 · '+sessions.length+'개 세션':project?.name}</small></div>
  {#if snapshot}<div class="toolbar-actions">
   {#if view==='canvas'}
    {#if project&&discoverProviders.length}<details class="session-import"><summary title="외부 세션 찾기">외부 세션 찾기</summary><div class="session-import-menu card">{#each discoverProviders as p}<button onclick={(event)=>{event.currentTarget.closest('details')?.removeAttribute('open');void action({type:'discover',project_key:project.id,provider:'codex',provider_id:p.id});}}>{p.name}</button>{/each}</div></details>{/if}
    <button class="primary new-work" onclick={()=>showModal(project?'new':'project')}><Icon name="plus" /><span>새 업무</span></button>
   {:else if view==='conversation'&&run}
    <button class="icon-button" aria-label="업무 맥락" aria-pressed={contextOpen} title="업무 맥락" onclick={()=>contextOpen=!contextOpen}><Icon name="context" /></button>
   {:else if view==='usage'}<button onclick={()=>action({type:'refresh_providers'})}>새로고침</button>{/if}
  </div>{/if}
 </header>
 {#if error}<div class="global-error" role="alert"><span>{error}</span><button aria-label="오류 닫기" class="icon-button" onclick={()=>error=''}>×</button></div>{/if}
 {#if needsAuth}
 <main class="login-screen"><form class="card login-card" onsubmit={(e)=>{e.preventDefault();void authenticate();}}><h1>서버 연결</h1><label>인증 토큰<input type="password" autocomplete="off" bind:value={token} required /></label><button class="primary">연결</button></form></main>
 {:else if !snapshot}
 <main class="login-screen"><div class="card login-card"><p>{loading?'연결 중…':'서버 연결 끊김'}</p><button onclick={connect} disabled={loading}>다시 연결</button></div></main>
 {:else}
 <main class={'main-content '+view}>
  {#if view==='canvas'}
   <div class="canvas-layout" class:has-selection={!!run}>
    <Canvas runs={sessions} {works} edges={canvasEdges} selected={sessions.find(r=>sessionId(r)===sessionId(run))?.id??selected} storageKey={'bibi:board:'+snapshot.server_id+':'+projectId} onselect={select} onopen={open} {halfLife} {floor} {uiScale} />
    <div class="canvas-inspector" inert={!run} aria-hidden={!run}>
     {#if run}<RunDetails {run} {work} host={snapshot.hosts.find(h=>h.id===run.host_id)} {now} onopen={()=>open(run.id)} onclose={()=>navigate({run:''})} onchanged={refreshSnapshot} />{/if}
    </div>
   </div>
  {:else if view==='conversation'}
   {#if run&&project}
    <div class="conversation-layout" class:has-context={contextOpen}>
     <section class="conversation-panel card">
      <div class="conversation-heading"><div><strong>{run.title}</strong><small>{providerName(run,snapshot.providers)} · {run.model||'모델 확인 대기'} · {shortId(sessionId(run))} · {run.host_id}</small></div><span class={'badge '+run.state}>{states[run.state]}</span><SessionActions {run} onchanged={refreshSnapshot} />
       {#if (isActive(run.state)||run.state==='queued')&&run.capabilities.interrupt.supported}<button class="danger-button" onclick={()=>action({type:'interrupt',run_id:run.id})}>중단</button>{/if}
      </div>
      <div bind:this={messagesPane} class="messages" aria-live="polite" onscroll={()=>{if(messagesPane)followTail=messagesPane.scrollHeight-messagesPane.scrollTop-messagesPane.clientHeight<96;}}>
       {#if detail?.run.id===run.id}
        {#each (detail.conversation??detail.messages) as message(message.id)}
         <article class={'message '+message.role}>
          <div class="message-meta"><span>{message.role==='user'?'사용자':message.role==='assistant'?run.role:message.role==='tool'?'도구':'시스템'}{message.id.startsWith('input:')?' · 전달됨':''}</span><time>{new Date(message.created_at).toLocaleTimeString('ko-KR',{hour:'2-digit',minute:'2-digit'})}</time></div>
          {#if message.role==='tool'}<details><summary>{({'bibi_consult':'전문가 문의','bibi_inbox':'회신 확인','bibi_report':'진행 보고','bibi_guild_read':'길드 조회','bibi_guild_record':'길드 기록','consult':'전문가 문의','inbox':'회신 확인','report':'진행 보고','guild_read':'길드 조회','guild_record':'길드 기록'} as Record<string,string>)[message.text.split('\n')[0]]??'실행 기록'}</summary><div class="message-text">{message.text}</div></details>
          {:else if message.role==='assistant'}<Markdown text={message.text} />{:else}<div class="message-text">{message.text}</div>{/if}
         </article>
        {/each}
        {#each detail.approvals.filter(a=>a.state==='pending') as approval(approval.id)}<ApprovalForm {approval} onrespond={async(id,value)=>{await command({type:'respond',approval_id:id,value});await loadDetail(run.id);}} />{/each}
        {#each (detail.inputs??[]).filter(i=>i.state!=='delivered') as input(input.id)}<p class="input-status"><span class="badge">{{accepted:'접수됨',sending:'전달 확인 중',delivered:'전달됨',failed:'전달 실패',uncertain:'확인 필요'}[input.state]??input.state}</span> {input.text}</p>{/each}
        {#if run.error}<p class="error">{run.error}</p>{/if}
        {#if run.state==='uncertain'}<label class="check recovery"><input type="checkbox" onchange={(e)=>{if(e.currentTarget.checked)void action({type:'resolve_run',run_id:run.id,confirmed_stopped:true});}} />이전 프로세스 종료와 파일 변경을 확인했습니다.</label>{/if}
        {#if run.activity}<p class="activity-line">● {run.activity.summary} · 보고 {age(run.activity.reported_at,now)}</p>{/if}
       {:else}<p class="muted">기록 불러오는 중…</p>{/if}
      </div>
      <Composer serverId={snapshot.server_id} {project} {run} work={work??null} hosts={snapshot.hosts} providers={snapshot.providers} modelHistory={snapshot.model_history} selection={snapshot.model_selection} onsettings={()=>showModal('settings')} onlocal={localCommand} onaccepted={accepted} />
     </section>
     {#if contextOpen}<aside class="context-panel card"><div class="row"><strong>맥락</strong><div class="row"><button onclick={editContext}>편집</button><button class="icon-button" aria-label="맥락 닫기" onclick={()=>contextOpen=false}>×</button></div></div><small>업무 v{work?.context_revision} · 실행 v{run.context_revision}</small>
      <h3 class="context-label">목표</h3><p>{work?.goal??run.context.goal}</p><h3 class="context-label">제약</h3><ul>{#each work?.constraints??run.context.constraints as constraint}<li>{constraint}</li>{/each}</ul>
      {#if work?.decisions.length}<h3 class="context-label">결정</h3>{#each work.decisions as d}<p>{d.text}<small>{d.source} · {d.revision}</small></p>{/each}{/if}
      {#if work?.performed_actions.length}<h3 class="context-label">이미 적용한 변경</h3>{#each work.performed_actions as d}<p>{d.text}<small>{d.source}</small></p>{/each}{/if}
      {#if run.context.references.length}<h3 class="context-label">참조 자료</h3>{#each run.context.references as reference}<p class="reference">{reference.text}<small>{reference.source}</small></p>{/each}{/if}
      {#if run.context.previous_answer_excerpt}<details><summary>이전 답변 발췌{run.context.excerpt_truncated?' · 일부':''}</summary><p class="prewrap">{run.context.previous_answer_excerpt}</p></details>{/if}
      <details><summary>실행 사용량</summary><dl><dt>입력 토큰</dt><dd>{run.stats.input_tokens??'확인 불가'}</dd><dt>캐시 입력</dt><dd>{run.stats.cached_input_tokens??'확인 불가'}</dd><dt>출력 토큰</dt><dd>{run.stats.output_tokens??'확인 불가'}</dd></dl></details>
      {#if detail?.inbox.length}<details><summary>수신함 · {detail.inbox.length}</summary>{#each detail.inbox as entry}<button class="inbox-item" onclick={()=>open(entry.from_run_id)}>{shortId(entry.from_run_id)} · {age(entry.created_at,now)}{entry.late?' · 늦은 결과':''}{entry.context_revision!==work?.context_revision?' · 이전 맥락':''}</button>{/each}</details>{/if}
     </aside>{/if}
    </div>
   {:else}<div class="empty-state"><p>선택한 실행 없음</p><button onclick={()=>navigate({view:'canvas'})}>캔버스 열기</button><button class="primary" onclick={()=>showModal(project?'new':'project')}>새 업무</button></div>{/if}
  {:else}
   <QuotaCards {quotas} {now} connections={snapshot.providers} expanded />
   <div class="card host-panel"><div class="row"><h2>호스트</h2><button disabled={!project} onclick={()=>showModal('host')}>호스트 연결</button></div>{#each snapshot.hosts as host}<div class="host-row"><div><strong>{host.name}</strong><small>{host.platform} · {snapshot.providers.filter(p=>p.host_id===host.id).map(p=>p.name).join(' · ')||'등록된 제공자 없음'}</small></div><span class={'badge '+(host.connected&&now-host.observed_at<30000?'connected':'warn')}>{host.connected&&now-host.observed_at<30000?'연결됨':'확인 필요'}</span><small>{age(host.observed_at,now)}</small>{#if host.error}<p class="error">{host.error}</p>{/if}</div>{/each}</div>
   <div class="card host-panel"><h2>연결 범위</h2><dl><dt>서버 주소</dt><dd>{endpoint}</dd><dt>서버</dt><dd>{snapshot.server_id}</dd><dt>실행</dt><dd>BiBi 실행 {snapshot.runs.filter(r=>r.origin==='managed').length} · 외부 {snapshot.runs.filter(r=>r.origin==='external').length}</dd><dt>관측 기준</dt><dd>이 서버에 연결된 호스트와 등록된 세션</dd></dl></div>
  {/if}
 </main>
 {/if}
 </div>
</div>

{#if sidebarDrawer&&compactNavigation}
 <dialog class="sidebar-drawer" use:modalDialog aria-label="사이드바" oncancel={(e)=>{e.preventDefault();sidebarDrawer=false;}} onclick={(e)=>{if(e.target===e.currentTarget){const box=e.currentTarget.getBoundingClientRect();if(e.clientX<box.left||e.clientX>box.right||e.clientY<box.top||e.clientY>box.bottom)sidebarDrawer=false;}}}>
  {@render sidebar()}
 </dialog>
{/if}

{#if modal}
<div class="modal-backdrop" role="presentation" onclick={(e)=>{if(e.target===e.currentTarget)closeModal();}}>
 <dialog class="modal card" use:modalDialog oncancel={(e)=>{e.preventDefault();closeModal();}} aria-label={modal==='project'?'프로젝트 추가':modal==='new'?'새 업무':modal==='context'?'업무 맥락':modal==='host'?'호스트 연결':'설정'} tabindex="-1">
  <div class="row"><h2>{modal==='project'?'프로젝트 추가':modal==='new'?'새 업무':modal==='context'?'업무 맥락':modal==='host'?'호스트 연결':'설정'}</h2><button class="icon-button" aria-label="닫기" onclick={closeModal}>×</button></div>
  {#if modal==='project'}<form onsubmit={(e)=>{e.preventDefault();void createProject();}}><label>프로젝트 이름<input bind:value={name} required /></label><label>호스트 작업 경로<input bind:value={workspace} required placeholder="/path/to/project" /></label><label>openguild 경로<input bind:value={guild} placeholder="선택" /></label><button class="primary">추가</button></form>
  {:else if modal==='host'}<form onsubmit={(e)=>{e.preventDefault();void registerHost();}}>
   <label>호스트 이름<input bind:value={hostName} required /></label><label>서버 주소<input type="url" bind:value={hostUrl} placeholder="https://host.example" required /></label>
   <label>호스트 인증 토큰<input type="password" autocomplete="off" bind:value={hostToken} required /></label><label>이 프로젝트의 호스트 작업 경로<input bind:value={hostWorkspace} required /></label>
   <label>호스트의 openguild 경로<input bind:value={hostGuild} placeholder="선택" /></label>{#if error}<p class="error" role="alert">{error}</p>{/if}<button class="primary" disabled={savingHost}>{savingHost?'연결 중…':'연결'}</button>
  </form>
  {:else if modal==='new'&&snapshot&&project}<Composer serverId={snapshot.server_id} {project} hosts={snapshot.hosts} providers={snapshot.providers} modelHistory={snapshot.model_history} selection={snapshot.model_selection} onsettings={()=>showModal('settings')} onlocal={localCommand} onaccepted={accepted} />
  {:else if modal==='context'&&work}<form onsubmit={(e)=>{e.preventDefault();void saveContext();}}><label>목표<textarea bind:value={contextGoal} required rows="4"></textarea></label><label>제약 · 한 줄에 하나<textarea bind:value={contextConstraints} rows="5"></textarea></label><button class="primary">저장</button></form>
  {:else if modal==='settings'}<ProviderSettings providers={snapshot?.providers??[]} onchanged={refreshSnapshot} /><label>UI 크기 · {Math.round(uiScale*100)}%<input type="range" min=".5" max="2" step=".05" bind:value={uiScale} oninput={appearance} /></label><button onclick={()=>{uiScale=1;appearance();}}>100%로 복원</button><div class="form-grid"><label>화살표 반감기 · 초<input type="number" min="1" max="3600" bind:value={halfLife} onchange={appearance} /></label><label>최소 불투명도<input type="range" min=".05" max=".5" step=".05" bind:value={floor} onchange={appearance} /></label></div>
   {#if snapshot?.removed_sessions.length}<details><summary>제거한 세션 · {sessionNodes(snapshot.removed_sessions).length}</summary>{#each sessionNodes(snapshot.removed_sessions) as removed}<div class="row removed-session"><span>{removed.title}</span><button onclick={async()=>{await action({type:'set_session_hidden',run_id:removed.id,hidden:false});await refreshSnapshot();}}>복원</button></div>{/each}</details>{/if}
   {#if isDesktop()}<form onsubmit={(e)=>{e.preventDefault();void changeConnection();}}><label>서버 주소<input type="url" bind:value={remoteUrl} placeholder="비워 두면 로컬" /></label><label>인증 토큰<input type="password" bind:value={remoteToken} autocomplete="off" /></label><button class="primary">서버 변경</button></form>{/if}
   <small class="endpoint">{endpoint}</small>{#if snapshot&&!isDesktop()}<button onclick={async()=>{await request('/auth/logout','POST');modal='';snapshot=null;await connect();}}>연결 해제</button>{/if}
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
 </dialog>
</div>
{/if}
