<script lang="ts">
import {onMount} from 'svelte';
import {ApiError,command,request,login,subscribe,connection,setConnection,isDesktop} from '$lib/api';
import type {Snapshot,Detail,Run,Project,Work,Event,Receipt,Quota,Approval} from '$lib/types';
import {product,providers,states,shortId,age,dateTime,isActive} from '$lib/format';
import {sessionId,sessionNodes,sessionEdges} from '$lib/sessions';
import Canvas from '$lib/components/Canvas.svelte';
import QuotaCards from '$lib/components/QuotaCards.svelte';
import Composer from '$lib/components/Composer.svelte';
import RunDetails from '$lib/components/RunDetails.svelte';
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
let halfLife=$state(30),floor=$state(.15),contextGoal=$state(''),contextConstraints=$state('');
let unsubscribe=()=>{},detailTimer:ReturnType<typeof setTimeout>|undefined,detailGeneration=0,detailDirty=0;
const project=$derived(snapshot?.projects.find(p=>p.id===projectId)??snapshot?.projects[0]);
const runs=$derived(snapshot?.runs.filter(r=>!project||r.project_key===project.id)??[]);
const works=$derived(snapshot?.works.filter(w=>!project||w.project_key===project.id)??[]);
const run=$derived(runs.find(r=>r.id===selected));
const work=$derived(works.find(w=>w.id===run?.work_id));
const edges=$derived(snapshot?.transmissions.filter(t=>runs.some(r=>r.id===t.to_run_id))??[]);
const quotas=$derived([...(snapshot?.quotas??[])].sort((a,b)=>['codex','claude','ollama','mock'].indexOf(a.provider)-['codex','claude','ollama','mock'].indexOf(b.provider)));
const sessions=$derived(sessionNodes(runs));
const canvasEdges=$derived(sessionEdges(runs,edges));
const history=$derived(sessions.filter(r=>r.work_id===run?.work_id));
onMount(()=>{
 try{const s=JSON.parse(localStorage.getItem('bibi:appearance')??'null');if(s){halfLife=s.halfLife??30;floor=s.floor??.15;}}catch{/* Defaults. */}
 void connect();const timer=setInterval(()=>{now=Date.now();},1000);
 return()=>{unsubscribe();clearInterval(timer);clearTimeout(detailTimer);};
});
function showDialog(node:HTMLDialogElement){node.showModal();return{destroy(){node.close();}};}
function restoreSelection(){
 if(!snapshot)return;
 try{const s=JSON.parse(localStorage.getItem('bibi:selection:'+snapshot.server_id)??'null');if(s){projectId=s.project;selected=s.run;}}catch{/* Fresh selection. */}
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
  case 'run':{const next=event.data as Run;const current=snapshot.runs.find(r=>r.id===selected);upsert(snapshot.runs,next);if(current&&next.continued_from===selected&&sessionId(next)===sessionId(current)){selected=next.id;remember();}break;}
  case 'work':upsert(snapshot.works,event.data as Work);break;
  case 'project':upsert(snapshot.projects,event.data as Project);break;
  case 'host':upsert(snapshot.hosts,event.data as Snapshot['hosts'][number]);break;
  case 'quota':upsert(snapshot.quotas,event.data as Quota);break;
  case 'quotas':{const q=event.data as {provider:string;host_id:string;quotas:Quota[]};snapshot.quotas=[...snapshot.quotas.filter(v=>v.provider!==q.provider||v.host_id!==q.host_id),...q.quotas];break;}
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
function select(id:string){followTail=true;selected=id;detail=null;remember();void loadDetail(id);}
function open(id:string){select(id);view='conversation';}
async function accepted(receipt:Receipt){
 modal='';snapshot=await request<Snapshot>('/api/snapshot');open(receipt.run_id);
}
async function createProject(){
 error='';try{
  const p=await command<Project>({type:'create_project',name,workspace,guild_path:guild||null,constraints:[]});
  if(snapshot)upsert(snapshot.projects,p);projectId=p.id;selected='';modal='new';name='';workspace='';guild='';remember();
 }catch(e){error=String(e instanceof Error?e.message:e);}
}
async function action(body:unknown){
 error='';try{await command(body);if(selected)await loadDetail(selected);}catch(e){error=String(e instanceof Error?e.message:e);}
}
function editContext(){if(!work)return;contextGoal=work.goal;contextConstraints=work.constraints.join('\n');modal='context';}
async function saveContext(){
 if(!work)return;error='';
 try{const updated=await command<Work>({type:'update_context',work_id:work.id,update:{...work,expected_revision:work.context_revision,goal:contextGoal,constraints:contextConstraints.split('\n').filter(s=>s.trim())}});
  if(snapshot)upsert(snapshot.works,updated);modal='';
 }catch(e){error=String(e instanceof Error?e.message:e);}
}
function projectChanged(){selected='';detail=null;remember();}
async function registerHost(){
 if(!project||savingHost)return;savingHost=true;error='';
 try{await command({type:'register_host',name:hostName,url:hostUrl,token:hostToken,project_key:project.id,workspace:hostWorkspace,guild_path:hostGuild||null});hostToken='';modal='';snapshot=await request<Snapshot>('/api/snapshot');}
 catch(e){error=e instanceof Error?e.message:String(e);}finally{savingHost=false;}
}
async function changeConnection(){
 unsubscribe();detailGeneration++;
 try{await setConnection(remoteUrl,remoteToken);remoteToken='';snapshot=null;selected='';detail=null;modal='';await connect();}catch(e){error=String(e);}
}
</script>

<svelte:head><title>{product.name}</title><meta name="description" content="BiBi 세션 캔버스" /></svelte:head>
<div class="app-shell">
 <header class="topbar"><button class="brand" onclick={()=>view='canvas'}>{product.name}</button><div class="divider"></div>
  {#if snapshot?.projects.length}<select aria-label="프로젝트" class="project-picker" bind:value={projectId} onchange={projectChanged}>{#each snapshot.projects as p}<option value={p.id}>{p.name}</option>{/each}</select>{:else}<span class="muted">프로젝트 없음</span>{/if}
  <div class="spacer"></div><button class="icon-button" aria-label="설정" onclick={()=>modal='settings'}>⚙</button>
 </header>
 {#if snapshot}
 <nav class="navigation" aria-label="주요 메뉴">
  <button class:active={view==='canvas'} onclick={()=>view='canvas'}>세션 캔버스</button>
  <button class:active={view==='conversation'} onclick={()=>view='conversation'}>작업 대화</button>
  <button class:active={view==='usage'} onclick={()=>view='usage'}>사용량·연결</button>
  <div class="spacer"></div><span class="connection-count">호스트 {snapshot.hosts.length} · 세션 {sessions.length}</span>
  <span class={'badge '+(connected?'connected':'warn')}>{connected?'연결됨':'재연결 중'}</span>
 </nav>
 {/if}
 {#if error}<div class="global-error" role="alert"><span>{error}</span><button aria-label="오류 닫기" class="icon-button" onclick={()=>error=''}>×</button></div>{/if}
 {#if needsAuth}
 <main class="login-screen"><form class="card login-card" onsubmit={(e)=>{e.preventDefault();void authenticate();}}><h1>서버 연결</h1><label>인증 토큰<input type="password" autocomplete="off" bind:value={token} required /></label><button class="primary">연결</button></form></main>
 {:else if !snapshot}
 <main class="login-screen"><div class="card login-card"><p>{loading?'연결 중…':'서버 연결 끊김'}</p><button onclick={connect} disabled={loading}>다시 연결</button></div></main>
 {:else}
 <main class={'main-content '+view}>
  {#if view==='canvas'}
   <QuotaCards {quotas} {now} />
   <div class="workspace-toolbar"><span>{works.length}개 업무</span><div class="spacer"></div><button onclick={()=>modal='project'}>프로젝트 추가</button><button class="primary" onclick={()=>modal=project?'new':'project'}>+ 새 업무</button></div>
   <div class="canvas-layout" class:has-selection={!!run}>
    <Canvas runs={sessions} {works} edges={canvasEdges} selected={sessions.find(r=>sessionId(r)===sessionId(run))?.id??selected} storageKey={'bibi:board:'+snapshot.server_id+':'+projectId} onselect={select} onopen={open} {halfLife} {floor} />
    {#if run}<RunDetails {run} {work} host={snapshot.hosts.find(h=>h.id===run.host_id)} {now} onopen={()=>open(run.id)} onclose={()=>{selected='';remember();}} />{/if}
   </div>
  {:else if view==='conversation'}
   {#if run&&project}
    <div class="page-heading"><div><h1>{work?.title??run.title}</h1><small>{shortId(run.work_id)} · {run.role}</small></div><button onclick={()=>view='canvas'}>캔버스</button></div>
    <div class="conversation-layout">
     <aside class="history card"><strong>세션</strong>{#each history as item(item.id)}<button class:active={sessionId(item)===sessionId(run)} onclick={()=>select(item.id)}><strong>{item.agent_kind==='subagent'?'서브에이전트':item.agent_kind==='expert'?'전문가':'세션'} {shortId(sessionId(item))}</strong><span>{item.title}</span><small>{providers[item.provider]} · {states[item.state]}</small></button>{/each}</aside>
     <section class="conversation-panel card">
      <div class="conversation-heading"><div><strong>{run.title}</strong><small>{providers[run.provider]} · {shortId(sessionId(run))} · {run.host_id}</small></div><span class={'badge '+run.state}>{states[run.state]}</span>
       {#if (isActive(run.state)||run.state==='queued')&&run.capabilities.interrupt.supported}<button class="danger-button" onclick={()=>action({type:'interrupt',run_id:run.id})}>중단</button>{/if}
      </div>
      <div bind:this={messagesPane} class="messages" aria-live="polite" onscroll={()=>{if(messagesPane)followTail=messagesPane.scrollHeight-messagesPane.scrollTop-messagesPane.clientHeight<96;}}>
       {#if detail?.run.id===run.id}
        {#each (detail.conversation??detail.messages) as message(message.id)}
         <article class={'message '+message.role}>
          <div class="message-meta"><span>{message.role==='user'?'사용자':message.role==='assistant'?run.role:message.role==='tool'?'도구':'시스템'}{message.id.startsWith('input:')?' · 전달됨':''}</span><time>{new Date(message.created_at).toLocaleTimeString('ko-KR',{hour:'2-digit',minute:'2-digit'})}</time></div>
          {#if message.role==='tool'}<details><summary>{({'bibi_consult':'전문가 문의','bibi_inbox':'회신 확인','bibi_report':'진행 보고','bibi_guild_read':'길드 조회','bibi_guild_record':'길드 기록','consult':'전문가 문의','inbox':'회신 확인','report':'진행 보고','guild_read':'길드 조회','guild_record':'길드 기록'} as Record<string,string>)[message.text.split('\n')[0]]??'실행 기록'}</summary><div class="message-text">{message.text}</div></details>
          {:else}<div class="message-text">{message.text}</div>{/if}
         </article>
        {/each}
        {#each detail.approvals.filter(a=>a.state==='pending') as approval(approval.id)}<ApprovalForm {approval} onrespond={async(id,value)=>{await command({type:'respond',approval_id:id,value});await loadDetail(run.id);}} />{/each}
        {#each (detail.inputs??[]).filter(i=>i.state!=='delivered') as input(input.id)}<p class="input-status"><span class="badge">{{accepted:'접수됨',sending:'전달 확인 중',delivered:'전달됨',failed:'전달 실패',uncertain:'확인 필요'}[input.state]??input.state}</span> {input.text}</p>{/each}
        {#if run.error}<p class="error">{run.error}</p>{/if}
        {#if run.state==='uncertain'}<label class="check recovery"><input type="checkbox" onchange={(e)=>{if(e.currentTarget.checked)void action({type:'resolve_run',run_id:run.id,confirmed_stopped:true});}} />이전 프로세스 종료와 파일 변경을 확인했습니다.</label>{/if}
        {#if run.activity}<p class="activity-line">● {run.activity.summary} · 보고 {age(run.activity.reported_at,now)}</p>{/if}
       {:else}<p class="muted">기록 불러오는 중…</p>{/if}
      </div>
      <Composer serverId={snapshot.server_id} {project} {run} work={work??null} hosts={snapshot.hosts} onaccepted={accepted} />
     </section>
     <aside class="context-panel card"><div class="row"><strong>맥락</strong><button onclick={editContext}>편집</button></div><small>업무 v{work?.context_revision} · 실행 v{run.context_revision}</small>
      <h3 class="context-label">목표</h3><p>{work?.goal??run.context.goal}</p><h3 class="context-label">제약</h3><ul>{#each work?.constraints??run.context.constraints as constraint}<li>{constraint}</li>{/each}</ul>
      {#if work?.decisions.length}<h3 class="context-label">결정</h3>{#each work.decisions as d}<p>{d.text}<small>{d.source} · {d.revision}</small></p>{/each}{/if}
      {#if work?.performed_actions.length}<h3 class="context-label">이미 적용한 변경</h3>{#each work.performed_actions as d}<p>{d.text}<small>{d.source}</small></p>{/each}{/if}
      {#if run.context.references.length}<h3 class="context-label">참조 자료</h3>{#each run.context.references as reference}<p class="reference">{reference.text}<small>{reference.source}</small></p>{/each}{/if}
      {#if run.context.previous_answer_excerpt}<details><summary>이전 답변 발췌{run.context.excerpt_truncated?' · 일부':''}</summary><p class="prewrap">{run.context.previous_answer_excerpt}</p></details>{/if}
      <details><summary>실행 사용량</summary><dl><dt>입력 토큰</dt><dd>{run.stats.input_tokens??'확인 불가'}</dd><dt>캐시 입력</dt><dd>{run.stats.cached_input_tokens??'확인 불가'}</dd><dt>출력 토큰</dt><dd>{run.stats.output_tokens??'확인 불가'}</dd></dl></details>
      {#if detail?.inbox.length}<details><summary>수신함 · {detail.inbox.length}</summary>{#each detail.inbox as entry}<button class="inbox-item" onclick={()=>open(entry.from_run_id)}>{shortId(entry.from_run_id)} · {age(entry.created_at,now)}{entry.late?' · 늦은 결과':''}{entry.context_revision!==work?.context_revision?' · 이전 맥락':''}</button>{/each}</details>{/if}
     </aside>
    </div>
   {:else}<div class="empty-state"><p>선택한 실행 없음</p><button onclick={()=>view='canvas'}>캔버스 열기</button><button class="primary" onclick={()=>modal=project?'new':'project'}>새 업무</button></div>{/if}
  {:else}
   <div class="workspace-toolbar"><span>{endpoint}</span><div class="spacer"></div><button onclick={()=>action({type:'refresh_providers'})}>사용량 갱신</button>{#if project}<button onclick={()=>action({type:'discover',project_key:project.id,provider:'codex'})}>Codex 외부 세션 찾기</button>{/if}</div>
   <QuotaCards {quotas} {now} expanded />
   <div class="card host-panel"><div class="row"><h2>호스트</h2><button disabled={!project} onclick={()=>modal='host'}>호스트 연결</button></div>{#each snapshot.hosts as host}<div class="host-row"><div><strong>{host.name}</strong><small>{host.platform} · {host.providers.map(p=>providers[p]).join(' · ')}</small></div><span class={'badge '+(host.connected&&now-host.observed_at<30000?'connected':'warn')}>{host.connected&&now-host.observed_at<30000?'연결됨':'확인 필요'}</span><small>{age(host.observed_at,now)}</small>{#if host.error}<p class="error">{host.error}</p>{/if}</div>{/each}</div>
   <div class="card host-panel"><h2>연결 범위</h2><dl><dt>서버</dt><dd>{snapshot.server_id}</dd><dt>실행</dt><dd>BiBi 실행 {snapshot.runs.filter(r=>r.origin==='managed').length} · 외부 {snapshot.runs.filter(r=>r.origin==='external').length}</dd><dt>관측 기준</dt><dd>이 서버에 연결된 호스트와 등록된 세션</dd></dl></div>
  {/if}
 </main>
 {/if}
</div>

{#if modal}
<div class="modal-backdrop" role="presentation" onclick={(e)=>{if(e.target===e.currentTarget)modal='';}}>
 <dialog class="modal card" use:showDialog onclose={()=>modal=''} oncancel={()=>modal=''} aria-label={modal==='project'?'프로젝트 추가':modal==='new'?'새 업무':modal==='context'?'업무 맥락':modal==='host'?'호스트 연결':'설정'} tabindex="-1" onkeydown={(e)=>{if(e.key==='Escape')modal='';}}>
  <div class="row"><h2>{modal==='project'?'프로젝트 추가':modal==='new'?'새 업무':modal==='context'?'업무 맥락':modal==='host'?'호스트 연결':'설정'}</h2><button class="icon-button" aria-label="닫기" onclick={()=>modal=''}>×</button></div>
  {#if modal==='project'}<form onsubmit={(e)=>{e.preventDefault();void createProject();}}><label>프로젝트 이름<input bind:value={name} required /></label><label>호스트 작업 경로<input bind:value={workspace} required placeholder="/path/to/project" /></label><label>openguild 경로<input bind:value={guild} placeholder="선택" /></label><button class="primary">추가</button></form>
  {:else if modal==='host'}<form onsubmit={(e)=>{e.preventDefault();void registerHost();}}>
   <label>호스트 이름<input bind:value={hostName} required /></label><label>서버 주소<input type="url" bind:value={hostUrl} placeholder="https://host.example" required /></label>
   <label>호스트 인증 토큰<input type="password" autocomplete="off" bind:value={hostToken} required /></label><label>이 프로젝트의 호스트 작업 경로<input bind:value={hostWorkspace} required /></label>
   <label>호스트의 openguild 경로<input bind:value={hostGuild} placeholder="선택" /></label>{#if error}<p class="error" role="alert">{error}</p>{/if}<button class="primary" disabled={savingHost}>{savingHost?'연결 중…':'연결'}</button>
  </form>
  {:else if modal==='new'&&snapshot&&project}<Composer serverId={snapshot.server_id} {project} hosts={snapshot.hosts} onaccepted={accepted} />
  {:else if modal==='context'&&work}<form onsubmit={(e)=>{e.preventDefault();void saveContext();}}><label>목표<textarea bind:value={contextGoal} required rows="4"></textarea></label><label>제약 · 한 줄에 하나<textarea bind:value={contextConstraints} rows="5"></textarea></label><button class="primary">저장</button></form>
  {:else if modal==='settings'}<div class="form-grid"><label>화살표 반감기 · 초<input type="number" min="1" max="3600" bind:value={halfLife} onchange={()=>localStorage.setItem('bibi:appearance',JSON.stringify({halfLife,floor}))} /></label><label>최소 불투명도<input type="range" min=".05" max=".5" step=".05" bind:value={floor} onchange={()=>localStorage.setItem('bibi:appearance',JSON.stringify({halfLife,floor}))} /></label></div>
   {#if isDesktop()}<form onsubmit={(e)=>{e.preventDefault();void changeConnection();}}><label>서버 주소<input type="url" bind:value={remoteUrl} placeholder="비워 두면 로컬" /></label><label>인증 토큰<input type="password" bind:value={remoteToken} autocomplete="off" /></label><button class="primary">서버 변경</button></form>{/if}
   <small class="endpoint">{endpoint}</small>{#if snapshot&&!isDesktop()}<button onclick={async()=>{await request('/auth/logout','POST');modal='';snapshot=null;await connect();}}>연결 해제</button>{/if}
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
 </dialog>
</div>
{/if}
