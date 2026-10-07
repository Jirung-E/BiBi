<script lang="ts">
import {conversationScroll,freezeOffscreenMessages} from '$lib/conversation-scroll';
import {onMount,untrack,tick} from 'svelte';
import {modalDialog} from '$lib/modal';
import {scrollbars} from '$lib/scrollbars';
import WindowControls from '$lib/components/WindowControls.svelte';
import {windowChrome} from '$lib/window-chrome';
import PanelResize from '$lib/components/PanelResize.svelte';
import {panelDefaults,readPanelSizes,type PanelName} from '$lib/panels';
import {surfaceFade,drawerReveal,disclosure,reveal} from '$lib/motion';
import {pushState,replaceState} from '$app/navigation';
import {page} from '$app/state';
import {readNavigation,navigationUrl,type Navigation} from '$lib/navigation';
import {restoreHistory,pushHistory,historyEntry,initialHistory,rememberHistory,type NavigationHistory} from '$lib/navigation-history';
import {ApiError,command,request,login,subscribe,connection,setConnection,isDesktop} from '$lib/api';
import type {Snapshot,Detail,Run,Project,Work,Event,Receipt,Quota,Approval,ProviderConfig,ModelHistory,ModelSelection} from '$lib/types';
import {product,providers,providerName,stateLabel,shortId,age,dateTime,isActive} from '$lib/format';
import {sessionId,sessionNodes,sessionEdges,disconnectedSessions} from '$lib/sessions';
import Canvas from '$lib/components/Canvas.svelte';
import Icon from '$lib/components/Icon.svelte';
import ProjectExtensions from '$lib/components/ProjectExtensions.svelte';
import QuotaCards from '$lib/components/QuotaCards.svelte';
import Composer from '$lib/components/Composer.svelte';
import RunDetails from '$lib/components/RunDetails.svelte';
import ProviderSettings from '$lib/components/ProviderSettings.svelte';
import ThemeSettings from '$lib/components/ThemeSettings.svelte';
import Markdown from '$lib/components/Markdown.svelte';
import SessionActions from '$lib/components/SessionActions.svelte';
import {groupIds,inGroup,UNGROUPED} from '$lib/session-groups';
import SessionSearch from '$lib/components/SessionSearch.svelte';
import SessionCleanup from '$lib/components/SessionCleanup.svelte';
import ApprovalForm from '$lib/components/ApprovalForm.svelte';
type ImportResult={imported:number;errors:{session:string;error:string}[];scope:{workspace:string;host:string;max_sessions:number;active_control:boolean}};
type ImportState={request:number;server:string;project:string;projectName:string;providerName:string;pending:boolean;result:ImportResult|null;error:string;syncError:string};
let importing=$state<ImportState|null>(null),importRequest=0;
let importFileInput=$state<HTMLInputElement>();
let fileTarget:{provider:ProviderConfig;project:string;server:string}|null=null;
const nativeImport=(provider:ProviderConfig)=>['codex','claude'].includes(provider.adapter);
let canvas=$state<{fitAll:()=>void}>();
let toolsOpen=$state(false),toolsView=$state<'actions'|'import'>('actions');
let toolsRoot=$state<HTMLDivElement>(),toolsToggle=$state<HTMLButtonElement>();
let removedSection=$state<HTMLDetailsElement>();
let refreshingUsage=$state(false),usageError=$state(''),usageRequest=0;
function closeTools(focus=false){toolsOpen=false;if(focus)toolsToggle?.focus();}
async function toggleTools(event:MouseEvent){
 if(toolsOpen){closeTools();return;}toolsView='actions';toolsOpen=true;
 if(event.detail===0){await tick();toolsRoot?.querySelector<HTMLButtonElement>('.canvas-tools-menu button:not(:disabled)')?.focus();}
}
async function showActions(){toolsView='actions';await tick();toolsRoot?.querySelector<HTMLButtonElement>('.canvas-menu-item:not(:disabled)')?.focus();}
async function showImports(){toolsView='import';await tick();toolsRoot?.querySelector<HTMLButtonElement>('.tools-subhead button')?.focus();}
async function showRemoved(){
 showModal('settings');await tick();
 if(removedSection){const summary=removedSection.querySelector('summary');if(!removedSection.open)summary?.click();removedSection.scrollIntoView({block:'nearest'});summary?.focus();}
}
async function refreshUsage(){
 if(refreshingUsage||!snapshot)return;
 const generation=++usageRequest,server=snapshot.server_id;refreshingUsage=true;usageError='';
 try{
  const result=await command<{error?:string}>({type:'refresh_providers'});
  if(result.error)throw new Error(result.error);
  const updated=await request<Snapshot>('/api/snapshot');
  if(generation===usageRequest&&snapshot?.server_id===server)snapshot=newestSnapshot(updated);
 }catch(e){if(generation===usageRequest)usageError=e instanceof Error?e.message:String(e);}
 finally{if(generation===usageRequest)refreshingUsage=false;}
}
let snapshot=$state<Snapshot|null>(null),detail=$state<Detail|null>(null);
let messagesPane=$state<HTMLDivElement>();
let conversationComposer=$state<{focus:()=>void}>();
let followTail=$state(true);
$effect(()=>{
 const last=(detail?.conversation??detail?.messages)?.at(-1),revision=last?.id+':'+last?.text;
 const pane=messagesPane;
 if(revision&&followTail&&pane){
  const frame=requestAnimationFrame(()=>{if(followTail&&messagesPane===pane)pane.scrollTo({top:pane.scrollHeight});});
  return()=>cancelAnimationFrame(frame);
 }
});
let selected=$state(''),projectId=$state(''),focusedWork=$state(''),view=$state<'canvas'|'conversation'|'usage'>('canvas');
let connected=$state(false),loading=$state(true),needsAuth=$state(false),error=$state(''),token=$state(''),now=$state(Date.now());
let modal=$state<Navigation['modal']>(''),searchQuery=$state('');
let name=$state(''),workspace=$state(''),guild=$state(''),remoteUrl=$state(''),remoteToken=$state(''),endpoint=$state(''),connectionMode=$state('');
let hostName=$state(''),hostUrl=$state(''),hostToken=$state(''),hostWorkspace=$state(''),hostGuild=$state(''),savingHost=$state(false);
let uiScale=$state(1),routeReady=$state(false);
let navigationHistory=$state<NavigationHistory>({id:'',index:0,last:0}),travelling=$state(false);
const canGoBack=$derived(routeReady&&!travelling&&navigationHistory.index>0);
const canGoForward=$derived(routeReady&&!travelling&&navigationHistory.index<navigationHistory.last);
function travel(direction:-1|1){
 if(direction<0?!canGoBack:!canGoForward)return;
 travelling=true;sidebarDrawer=false;closeTools();window.history.go(direction);
}
// A browser on a Mac keeps browser chrome; only the native Mac window uses an overlay.
const macWindow=isDesktop()&&navigator.platform.startsWith('Mac');
const windowsWindow=isDesktop()&&navigator.platform.startsWith('Win');
const baseFont=navigator.platform.startsWith('Win')?16:14;
const fontSize=$derived(baseFont*uiScale);
const canvasScale=$derived(fontSize/14);
const customTitlebar=macWindow||windowsWindow;
let windowWidth=$state(0),sidebarOpen=$state(true),sidebarDrawer=$state(false),contextOpen=$state(false);
let workspaceWidth=$state(0),contentHeight=$state(0),resizing=$state(false);
let panels=$state({...panelDefaults});
const sidebarMax=$derived(Math.max(14,Math.min(26,windowWidth/fontSize-32)));
const sidebarWidth=$derived(Math.min(panels.sidebar,sidebarMax));
// Wide, shallow windows retain a side panel instead of two unreadably short rows.
const inspectorStacked=$derived(workspaceWidth<=48*fontSize&&!(workspaceWidth>=36*fontSize&&contentHeight<=16*fontSize));
const contextStacked=$derived(workspaceWidth<=60*fontSize);
const detailMax=$derived(Math.max(18,Math.min(32,workspaceWidth/fontSize-24)));
// Leave a readable body below fixed panel controls at enlarged mobile text sizes.
const detailHeightMax=$derived(Math.max(0,Math.min(30,Math.max(10,(contentHeight/fontSize-1)*.55),contentHeight/fontSize-4.75)));
const inspectorWidth=$derived(Math.min(panels.inspector,detailMax));
const contextWidth=$derived(Math.min(panels.context,detailMax));
const inspectorHeight=$derived(Math.min(panels.inspectorHeight,detailHeightMax));
const contextHeight=$derived(Math.min(panels.contextHeight,detailHeightMax));
function savePanels(){try{localStorage.setItem('bibi:panels',JSON.stringify(panels));}catch{/* Optional device preference. */}}
function resizePanel(name:PanelName,value:number){panels[name]=value;}
function resetPanel(name:PanelName){panels[name]=panelDefaults[name];savePanels();}
let sidebarToggle:HTMLButtonElement;
function closeSidebar(){sidebarDrawer=false;}
const compactNavigation=$derived(windowWidth<1000*canvasScale);
function toggleSidebar(){
 freezeOffscreenMessages(messagesPane);
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
const runIds=$derived(new Set(runs.map(r=>r.id)));
const edges=$derived(snapshot?.transmissions.filter(t=>runIds.has(t.to_run_id))??[]);
const quotas=$derived.by(()=>{
 const values=(snapshot?.quotas??[]).filter(q=>snapshot?.providers.some(p=>p.id===q.provider_id));
 return [...values,...(snapshot?.providers??[]).filter(p=>!values.some(q=>q.provider_id===p.id)).map(p=>({id:'pending:'+p.id,provider:p.adapter,provider_id:p.id,account:p.name,host_id:p.host_id,model:null,status:'unknown',windows:[],observed_at:null,reason:'사용량 갱신 대기'} as Quota))];
});
const discoverProviders=$derived(snapshot?.providers.filter(p=>p.adapter!=='mock'&&p.host_id==='local')??[]);
const importStatus=$derived(importing?.server===snapshot?.server_id&&importing?.project===project?.id?importing:null);
const sessions=$derived(sessionNodes(runs));
const cleanupCandidates=$derived(disconnectedSessions(runs,snapshot?.approvals??[]));
const canvasEdges=$derived(sessionEdges(runs,edges));
const memberships=$derived(snapshot?.session_groups??[]);
const historyGroup=$derived(focusedWork||(run?groupIds(run,memberships)[0]??UNGROUPED:''));
const sessionHistory=$derived(sessions.filter(r=>inGroup(r,historyGroup,memberships)));
onMount(()=>{
 const viewport=window.visualViewport;
 const resize=()=>{if(viewport?.scale===1)document.documentElement.style.setProperty('--app-height',viewport.height+'px');else document.documentElement.style.removeProperty('--app-height');};
 resize();viewport?.addEventListener('resize',resize);
 try{const s=JSON.parse(localStorage.getItem('bibi:appearance')??'null');if(s){halfLife=s.halfLife??30;floor=s.floor??.15;uiScale=Math.max(.5,Math.min(2,s.uiScale??1));}}catch{/* Defaults. */}
 document.documentElement.style.fontSize=fontSize+'px';
 // Keep the same 44px/16px touch controls at 100% on either base font.
 document.documentElement.style.setProperty('--touch-control-size',44/baseFont+'rem');
 document.documentElement.style.setProperty('--touch-control-font-size',16/baseFont+'rem');
 try{sidebarOpen=JSON.parse(localStorage.getItem('bibi:sidebar')??'true')!==false;}catch{/* Default expanded. */}
 try{panels=readPanelSizes(localStorage.getItem('bibi:panels'));}catch{/* Defaults. */}
 const outside=(event:globalThis.Event)=>{if(toolsOpen&&event.target instanceof Node&&!toolsRoot?.contains(event.target))closeTools();};
 const escape=(event:KeyboardEvent)=>{if((event.metaKey||event.ctrlKey)&&event.key.toLowerCase()==='k'&&snapshot&&project){event.preventDefault();showModal('search');}if(toolsOpen&&event.key==='Escape'){event.preventDefault();closeTools(true);}};
 document.addEventListener('pointerdown',outside);document.addEventListener('focusin',outside);document.addEventListener('keydown',escape);
 void connect();const timer=setInterval(()=>{now=Date.now();if(now-lastStatusCheck>=3000)void reconcileStatus();},1000);
 return()=>{usageRequest++;document.removeEventListener('pointerdown',outside);document.removeEventListener('focusin',outside);document.removeEventListener('keydown',escape);unsubscribe();clearInterval(timer);clearTimeout(detailTimer);viewport?.removeEventListener('resize',resize);document.documentElement.style.removeProperty('--app-height');};
});
function currentNavigation():Navigation{return {view,project:projectId,group:focusedWork,run:selected,modal};}
function applyNavigation(next:Navigation){
 if(next.view!==view||next.project!==projectId||next.modal!==modal)closeTools();
 if(next.run&&snapshot?.removed_sessions.some(r=>r.id===next.run)){
  next={...next,run:''};replaceState(navigationUrl(new URL(window.location.href),next),{...page.state,bibi:next});
 }
 const changed=selected!==next.run;view=next.view;projectId=next.project;focusedWork=next.group??'';selected=next.run;modal=next.modal;
 if(changed){detail=null;followTail=true;if(selected)void loadDetail(selected);}
 if(modal==='context'&&work){contextGoal=work.goal;contextConstraints=work.constraints.join('\n');}remember();
}
function navigate(change:Partial<Navigation>,replace=false){
 sidebarDrawer=false;closeTools();
 const next={...currentNavigation(),...change};const current=new URL(window.location.href);const url=navigationUrl(current,next);
 if(url.href!==current.href){
  if(!replace)navigationHistory=pushHistory(navigationHistory);
  (replace?replaceState:pushState)(url,{...page.state,bibi:next,bibiHistory:historyEntry(navigationHistory),bibiModal:!!next.modal&&(!replace||!!(page.state as {bibiModal?:boolean}).bibiModal)});
 }
 applyNavigation(next);
}
$effect(()=>{
 const state=page.state as {bibi?:Navigation;bibiHistory?:unknown};
 if(routeReady&&state.bibi)untrack(()=>{
  travelling=false;navigationHistory=restoreHistory(state.bibiHistory,navigationHistory);
  sidebarDrawer=false;applyNavigation(state.bibi!);
  rememberHistory(navigationHistory,!!(page.state as {bibiModal?:boolean}).bibiModal);
 });
});
function showModal(next:Navigation['modal']){navigate({modal:next});}
function closeModal(){if(!modal)return;if((page.state as {bibiModal?:boolean}).bibiModal&&canGoBack)travel(-1);else navigate({modal:''},true);}
function restoreSelection(){
 if(!snapshot)return;
 const url=new URL(window.location.href);let next=readNavigation(url);
 if(!url.searchParams.has('view')){try{const saved=JSON.parse(localStorage.getItem('bibi:selection:'+snapshot.server_id)??'null');if(saved){next.project=saved.project??'';next.run=saved.run??'';}}catch{/* Empty selection. */}}
 if(!snapshot.projects.some(p=>p.id===next.project))next.project=snapshot.projects[0]?.id??'';
 if(!snapshot.works.some(w=>w.id===next.group&&w.project_key===next.project))next.group='';
 if(!snapshot.runs.some(r=>r.id===next.run&&r.project_key===next.project))next.run='';
 const initial=initialHistory(),state=page.state as {bibiHistory?:unknown;bibiModal?:boolean};
 navigationHistory=restoreHistory(state.bibiHistory??initial?.entry);
 const bibiModal=!!(state.bibiModal??initial?.modal)&&navigationHistory.index>0;
 applyNavigation(next);replaceState(navigationUrl(url,next),{...page.state,bibi:next,bibiHistory:historyEntry(navigationHistory),bibiModal});routeReady=true;
}
function appearance(){uiScale=Math.max(.5,Math.min(2,uiScale||1));requestAnimationFrame(()=>document.documentElement.style.fontSize=fontSize+'px');localStorage.setItem('bibi:appearance',JSON.stringify({halfLife,floor,uiScale}));}
function newestSnapshot(next:Snapshot):Snapshot{
 // An HTTP response may arrive after newer stream events, especially on mobile.
 return snapshot&&snapshot.server_id===next.server_id&&snapshot.last_seq>next.last_seq?snapshot:next;
}
async function refreshSnapshot(){snapshot=newestSnapshot(await request<Snapshot>('/api/snapshot'));if(selected&&!snapshot.runs.some(r=>r.id===selected))navigate({run:''},true);if(selected)await loadDetail(selected);}
async function localCommand(name:string,arg:string){
 if(name==='resume'){searchQuery=arg;showModal('search');return;}
 if(name==='new'||name==='clear'){showModal(project?'new':'project');return;}
 if(name==='usage'){navigate({view:'usage',modal:''});await refreshUsage();return;}
 if(name==='extensions'){showModal('extensions');return;}
 if(name==='rename'){if(!run||!arg.trim())throw new Error('/bibi rename 새 이름 형식으로 입력하세요.');await command({type:'rename_session',run_id:run.id,title:arg});await refreshSnapshot();}
}
function remember(){if(snapshot)localStorage.setItem('bibi:selection:'+snapshot.server_id,JSON.stringify({project:project?.id,run:selected}));}
async function connect(){
 loading=true;error='';unsubscribe();importRequest++;importing=null;usageRequest++;refreshingUsage=false;usageError='';closeTools();
 try{
  const info=await connection();endpoint=info.url;connectionMode=info.mode;
  snapshot=newestSnapshot(await request<Snapshot>('/api/snapshot'));needsAuth=false;connected=true;restoreSelection();
  if(!snapshot.projects.some(p=>p.id===projectId))projectId=snapshot.projects[0]?.id??'';
  if(!snapshot.runs.some(r=>r.id===selected&&r.project_key===projectId))selected='';
  unsubscribe=subscribe(snapshot.last_seq,update,v=>connected=v);
  if(selected)void loadDetail(selected);
 }catch(e){needsAuth=e instanceof ApiError&&e.status===401;error=String(e instanceof Error?e.message:e);connected=false;}
 finally{loading=false;}
}
async function authenticate(){error='';try{await login(token);token='';await connect();}catch(e){error=String(e instanceof Error?e.message:e);}}
function upsert<T extends {id:string}>(list:T[],value:T){const index=list.findIndex(item=>item.id===value.id);if(index<0)list.push(value);else list[index]=value;}
function mergeRun(list:Run[],next:Run){
 const prior=list.find(r=>r.id===next.id);
 const terminal=(r:Run)=>['completed','failed','interrupted','uncertain'].includes(r.state);
 if(!prior||next.updated_at>prior.updated_at||(next.updated_at===prior.updated_at&&(!terminal(prior)||terminal(next))))upsert(list,next);
}
function update(event:Event){
 if(!snapshot||event.seq<=snapshot.last_seq)return;
 snapshot.last_seq=event.seq;
 switch(event.kind){
  case 'run':{const next=event.data as Run;const current=snapshot.runs.find(r=>r.id===selected);const list=snapshot.removed_sessions.some(r=>sessionId(r)===sessionId(next))?snapshot.removed_sessions:snapshot.runs;mergeRun(list,next);if(current&&next.continued_from===selected&&sessionId(next)===sessionId(current)){navigate({run:next.id},true);}break;}
  case 'provider':upsert(snapshot.providers,event.data as ProviderConfig);break;
  case 'provider_deleted':{const id=(event.data as {id:string}).id;snapshot.providers=snapshot.providers.filter(p=>p.id!==id);snapshot.quotas=snapshot.quotas.filter(q=>q.provider_id!==id);if(snapshot.model_selection?.provider_id===id)snapshot.model_selection=null;break;}
  case 'model_selection':snapshot.model_selection=event.data as ModelSelection;break;
  case 'model_history':{const item=event.data as ModelHistory;snapshot.model_history=[...snapshot.model_history.filter(h=>h.provider_id!==item.provider_id||h.model!==item.model),item];break;}
  case 'session_visibility':void refreshSnapshot();break;
  case 'session_groups':{const item=event.data as NonNullable<Snapshot['session_groups']>[number];snapshot.session_groups=[...(snapshot.session_groups??[]).filter(g=>g.session_id!==item.session_id),item];break;}
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
let statusBusy=false,lastStatusCheck=0;
async function reconcileStatus(){
 if(statusBusy||!snapshot||!run||!['queued','running','waiting_user','waiting_expert','disconnected','uncertain'].includes(run.state)||document.visibilityState==='hidden')return;
 const id=selected,server=snapshot.server_id;statusBusy=true;lastStatusCheck=Date.now();
 try{
  const next=await request<Run>('/api/runs/'+id+'/status');
  if(snapshot?.server_id!==server||selected!==id||!next?.id)return;
  const previous=snapshot.runs.find(r=>r.id===id);
  mergeRun(snapshot.runs,next);
  if(previous&&(previous.state!==next.state||previous.updated_at!==next.updated_at))await loadDetail(id);
 }catch{/* The event stream owns connection errors; retry without replacing the draft. */}
 finally{statusBusy=false;}
}
async function loadDetail(id:string){
 const generation=++detailGeneration,dirty=detailDirty;
 try{
  const value=await request<Detail>('/api/runs/'+id);
  if(selected===id&&generation===detailGeneration){
   detail=value;
   if(snapshot&&value.run.project_key===projectId&&detailDirty===dirty)mergeRun(snapshot.runs,value.run);
   if(detailDirty!==dirty&&!detailTimer)detailTimer=setTimeout(()=>{detailTimer=undefined;void loadDetail(id);},150);
  }
 }catch(e){if(selected===id)error=String(e instanceof Error?e.message:e);}
}
function targetGroup(id:string){const target=runs.find(r=>r.id===id);return focusedWork&&target&&inGroup(target,focusedWork,memberships)?focusedWork:'';}
function select(id:string){navigate({run:id,group:targetGroup(id)});}
function open(id:string){navigate({run:id,view:'conversation',group:targetGroup(id)});}
async function accepted(receipt:Receipt){
 const previous=run;snapshot=newestSnapshot(await request<Snapshot>('/api/snapshot'));const next=snapshot.runs.find(r=>r.id===receipt.run_id);
 const focusNewConversation=modal==='new'&&document.activeElement?.matches('.composer textarea');
 navigate({run:receipt.run_id,view:'conversation',modal:''},!!modal||sessionId(previous)===sessionId(next));
 if(focusNewConversation){await tick();conversationComposer?.focus();}
}
async function createProject(){
 error='';try{
  const p=await command<Project>({type:'create_project',name,workspace,guild_path:guild||null,constraints:[]});
  if(snapshot)upsert(snapshot.projects,p);navigate({project:p.id,group:'',run:'',modal:'new'},true);name='';workspace='';guild='';remember();
 }catch(e){error=String(e instanceof Error?e.message:e);}
}
async function action(body:unknown){
 error='';try{const result=await command<Partial<Run>>(body);if(snapshot&&result?.id&&result.state)mergeRun(snapshot.runs,result as Run);if(selected)await loadDetail(selected);}catch(e){error=String(e instanceof Error?e.message:e);}
}
function chooseImport(provider:ProviderConfig){
 if(nativeImport(provider)){void discoverExternal(provider);return;}
 if(!project||!snapshot||!importFileInput)return;
 fileTarget={provider,project:project.id,server:snapshot.server_id};
 importFileInput.value='';importFileInput.click();
}
async function importFile(){
 const target=fileTarget,file=importFileInput?.files?.[0];fileTarget=null;
 if(!target||!file||target.project!==project?.id||target.server!==snapshot?.server_id)return;
 await discoverExternal(target.provider,file);
}
async function discoverExternal(provider:ProviderConfig,file?:File){
 if(!project||!snapshot||importing?.pending)return;
 const current:ImportState={request:++importRequest,server:snapshot.server_id,project:project.id,projectName:project.name,providerName:provider.name,pending:true,result:null,error:'',syncError:''};
 importing=current;
 try{
  if(file&&file.size>8*1024*1024)throw new Error('대화 파일은 8 MiB 이하만 가져올 수 있습니다.');
  const result=file
   ?await request<ImportResult>('/api/import','POST',{project_key:current.project,provider_id:provider.id,document:await file.text()})
   :await command<ImportResult>({type:'discover',project_key:current.project,provider:provider.adapter,provider_id:provider.id});
  if(importRequest!==current.request)return;
  importing={...current,result};
  await refreshImported();
 }catch(e){if(importRequest===current.request)importing={...current,error:e instanceof Error?e.message:String(e)};}
 finally{if(importing?.request===current.request)importing.pending=false;}
}
async function refreshImported(){
 const current=importing;if(!current||current.server!==snapshot?.server_id)return;
 importing={...current,pending:true,syncError:''};
 try{await refreshSnapshot();}
 catch(e){if(importing?.request===current.request)importing.syncError='목록 갱신 실패: '+(e instanceof Error?e.message:String(e));}
 finally{if(importing?.request===current.request)importing.pending=false;}
}
async function showImported(){navigate({view:'canvas',group:'',run:''});await tick();canvas?.fitAll();}
function importSummary(result:ImportResult){
 if(result.errors.length)return result.imported?`세션 ${result.imported}개 가져옴 · ${result.errors.length}개 실패`:`세션 ${result.errors.length}개 가져오기 실패`;
 return result.imported?`세션 ${result.imported}개 가져옴`:'가져올 외부 세션이 없습니다.';
}
function editContext(){if(!work)return;contextGoal=work.goal;contextConstraints=work.constraints.join('\n');showModal('context');}
async function saveContext(){
 if(!work)return;error='';
 try{const updated=await command<Work>({type:'update_context',work_id:work.id,update:{...work,expected_revision:work.context_revision,goal:contextGoal,constraints:contextConstraints.split('\n').filter(s=>s.trim())}});
  if(snapshot)upsert(snapshot.works,updated);closeModal();
 }catch(e){error=String(e instanceof Error?e.message:e);}
}
function projectChanged(){navigate({project:projectId,group:'',run:''});}
async function registerHost(){
 if(!project||savingHost)return;savingHost=true;error='';
 try{await command({type:'register_host',name:hostName,url:hostUrl,token:hostToken,project_key:project.id,workspace:hostWorkspace,guild_path:hostGuild||null});hostToken='';closeModal();snapshot=newestSnapshot(await request<Snapshot>('/api/snapshot'));}
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
 <div class="sidebar-head" data-tauri-drag-region={customTitlebar?'':undefined}><button class="brand" onclick={()=>navigate({view:'canvas'})}>{product.name}</button><button class="icon-button" aria-label="사이드바 닫기" title="사이드바 닫기" onclick={()=>{if(compactNavigation)void closeSidebar();else toggleSidebar();}}><Icon name="sidebar" /></button></div>
 <div class="sidebar-scroll" use:scrollbars aria-label="탐색">
 {#if snapshot}
  <div class="sidebar-project"><div class="sidebar-section-head"><span class="sidebar-label">프로젝트</span><button class="icon-button" aria-label="프로젝트 확장 설정" title="스킬·플러그인·MCP" disabled={!project} onclick={()=>showModal('extensions')}><Icon name="settings" /></button></div><div class="project-controls">
   {#if snapshot.projects.length}<div class="project-select"><select aria-label="프로젝트" class="project-picker" bind:value={projectId} onchange={projectChanged}>{#each snapshot.projects as p}<option value={p.id}>{p.name}</option>{/each}</select></div>{:else}<span class="muted">프로젝트 없음</span>{/if}
   <button class="icon-button project-add" aria-label="프로젝트 추가" title="프로젝트 추가" onclick={()=>showModal('project')}><Icon name="plus" /></button>
  </div></div>
  <nav class="navigation" aria-label="주요 메뉴">
   {#if compactNavigation}<button aria-label="세션 검색" onclick={()=>showModal('search')}><Icon name="search" /><span>세션 검색</span></button>{/if}
   <button class:active={view==='canvas'} aria-current={view==='canvas'?'page':undefined} onclick={()=>navigate({view:'canvas'})}><Icon name="canvas" /><span>세션 캔버스</span><small aria-hidden="true">{sessions.length}</small></button>
   <button class:active={view==='conversation'} aria-current={view==='conversation'?'page':undefined} onclick={()=>navigate({view:'conversation'})}><Icon name="chat" /><span>작업 대화</span></button>
   <button class:active={view==='usage'} aria-current={view==='usage'?'page':undefined} onclick={()=>navigate({view:'usage'})}><Icon name="usage" /><span>사용량·연결</span></button>
  </nav>
  <div class="sidebar-sections">
   {#if view==='conversation'&&run}<section class="sidebar-section history" aria-label="업무 세션" transition:reveal><h2 class="sidebar-label">세션</h2>{#each sessionHistory as item(item.id)}<button class:active={sessionId(item)===sessionId(run)} onclick={()=>select(item.id)}><span>{item.title}</span><small>{item.agent_kind==='subagent'?'서브에이전트':providerName(item,snapshot.providers)} · {stateLabel(item)}</small></button>{/each}</section>{/if}
   {#if quotas.length}<section class="sidebar-section sidebar-usage"><div class="sidebar-section-head"><h2 class="sidebar-label">사용량</h2><button class="icon-button usage-refresh" class:refreshing={refreshingUsage} disabled={refreshingUsage} aria-label="사용량 새로고침" aria-busy={refreshingUsage} title={refreshingUsage?'사용량 갱신 중':'사용량 새로고침 · 5분마다 자동 갱신'} onclick={refreshUsage}><Icon name="refresh" /></button></div>{#if usageError}<small class="error usage-refresh-error" role="alert">{usageError}</small>{/if}<QuotaCards {quotas} {now} connections={snapshot.providers} compact onopen={()=>navigate({view:'usage'})} /></section>{/if}
  </div>
 {/if}
 </div>
 <div class="sidebar-footer">
  {#if snapshot}<div class="sidebar-connection" title={endpoint}><span class={'connection-dot '+(connected?'connected':'warn')} aria-hidden="true"></span><small>{connectionMode==='local'?'로컬 · 원격 접속 꺼짐 · ':connectionMode==='local-server'?'별도 로컬 서버 · ':connectionMode==='remote'?'원격 서버 · ':''}{connected?'연결됨':'재연결 중'}{#if connectionMode==='browser'} · 호스트 {snapshot.hosts.length}{/if}</small></div>{/if}
  <button class="settings-button" onclick={()=>showModal('settings')}><Icon name="settings" /><span>설정</span></button>
 </div>
{/snippet}

<div class="app-shell" class:mac-window={macWindow} class:windows-window={windowsWindow} class:sidebar-expanded={!compactNavigation&&sidebarOpen} class:canvas-view={!!snapshot&&view==='canvas'} class:panel-resizing={resizing} style:--sidebar-width={sidebarWidth+'rem'} style:--inspector-width={inspectorWidth+'rem'} style:--context-width={contextWidth+'rem'} style:--inspector-height={inspectorHeight+'rem'} style:--context-height={contextHeight+'rem'}>
 {#if macWindow}<span class="native-controls-anchor" aria-hidden="true"></span>{/if}
 <div class="sidebar-slot" inert={compactNavigation||!sidebarOpen} aria-hidden={compactNavigation||!sidebarOpen}>
  <aside class="app-sidebar" aria-label="사이드바">{#if !compactNavigation}{@render sidebar()}{/if}</aside>
  {#if !compactNavigation&&sidebarOpen}<PanelResize label="사이드바 너비" value={sidebarWidth} min={14} max={sidebarMax} unit={fontSize} onresize={v=>resizePanel('sidebar',v)} onactive={v=>resizing=v} oncommit={savePanels} onreset={()=>resetPanel('sidebar')} />{/if}
 </div>
 <div class="workspace-shell" bind:clientWidth={workspaceWidth}>
 <header class="content-toolbar" use:windowChrome={macWindow} data-tauri-drag-region={customTitlebar?'':undefined}>
  <button bind:this={sidebarToggle} class="icon-button sidebar-toggle" aria-label="사이드바 열기" aria-expanded={compactNavigation?sidebarDrawer:sidebarOpen} title="사이드바" onclick={toggleSidebar}><Icon name="sidebar" /></button>
  <div class="history-navigation" role="group" aria-label="탐색 기록">
   <button class="icon-button" aria-label="뒤로가기" title="뒤로가기" disabled={!canGoBack} onclick={()=>travel(-1)}><Icon name="chevron-left" /></button>
   <button class="icon-button" aria-label="앞으로가기" title="앞으로가기" disabled={!canGoForward} onclick={()=>travel(1)}><Icon name="chevron-right" /></button>
  </div>
  <div class="toolbar-title" data-tauri-drag-region={customTitlebar?'':undefined}><h1 data-tauri-drag-region={customTitlebar?'':undefined}>{view==='canvas'?'세션 캔버스':view==='conversation'?(work?.title??'작업 대화'):'사용량·연결'}</h1><small title={project?.name} data-tauri-drag-region={customTitlebar?'':undefined}>{view==='canvas'?works.length+'개 업무 · '+sessions.length+'개 세션':project?.name}</small></div>
  {#if snapshot}<div class="toolbar-actions">
   {#if !compactNavigation}<button class="icon-button" aria-label="세션 검색" title="세션 검색 · ⌘/Ctrl K" disabled={!project} onclick={()=>showModal('search')}><Icon name="search" /></button>{/if}
   {#if view==='canvas'}
    <div class="canvas-tools" bind:this={toolsRoot}>
     <button bind:this={toolsToggle} class="icon-button" aria-label="캔버스 메뉴" title="캔버스 메뉴" aria-expanded={toolsOpen} aria-controls="canvas-tools-menu" aria-haspopup="true" onclick={toggleTools}><Icon name="more" /></button>
     {#if toolsOpen}<div id="canvas-tools-menu" class="canvas-tools-menu card" class:session-import-menu={toolsView==='import'} role="group" aria-label={toolsView==='import'?'외부 세션 제공자':'캔버스 메뉴 항목'} use:scrollbars transition:surfaceFade>
      {#if toolsView==='actions'}
       <button class="canvas-menu-item" aria-label="외부 세션 가져오기" disabled={!project||!discoverProviders.length} onclick={showImports}><Icon name="import" /><span>가져오기</span></button>
       <button class="canvas-menu-item" aria-label="연결 끊긴 세션 정리" disabled={!project||!cleanupCandidates.length} onclick={()=>showModal('cleanup')}><Icon name="cleanup" /><span>연결 끊긴 세션 정리</span>{#if cleanupCandidates.length}<small>{cleanupCandidates.length}</small>{/if}</button>
      {:else if project}
      <div class="tools-subhead"><button class="icon-button" aria-label="캔버스 메뉴로 돌아가기" onclick={showActions}><Icon name="back" /></button><strong>가져오기</strong></div>
      <input bind:this={importFileInput} type="file" accept=".json,application/json" hidden aria-label="대화 JSON 파일" onchange={importFile} />
      {#each discoverProviders as p}<button disabled={importing?.pending} aria-label={p.name} title={nativeImport(p)?'저장된 세션 조회':'대화 JSON 파일 가져오기'} onclick={()=>chooseImport(p)}><span>{p.name}</span>{#if !nativeImport(p)}<small>대화 파일</small>{/if}</button>{/each}
      {#if discoverProviders.some(p=>!nativeImport(p))}<details class="import-scope" use:disclosure><summary>파일 형식</summary><small>텍스트 대화 JSON · 최대 8 MiB. 가져온 복사본에서 대화를 이어갑니다.</small><code>{JSON.stringify({title:"대화 이름",model:"모델명",messages:[{role:"user",content:"질문"},{role:"assistant",content:"답변"}]})}</code></details>{/if}
      {#if importStatus||importing?.pending}<div class="import-feedback" transition:reveal>
       <p role="status">{#if importing?.pending}{importStatus?importStatus.providerName+' 세션 조회 중…':importing.projectName+'에서 조회 중…'}{:else if importStatus?.error}{importStatus.providerName} 가져오기 실패{:else if importStatus?.result}{importSummary(importStatus.result)}{/if}</p>
       {#if importStatus?.error}<p class="error" role="alert">{importStatus.error}</p>{/if}
       {#if importStatus?.result&&!importStatus.pending}
        {#if importStatus.result.errors.length}<details use:disclosure><summary>실패 원인 {importStatus.result.errors.length}개</summary><ul>{#each importStatus.result.errors as item}<li><code>{item.session}</code><span>{item.error}</span></li>{/each}</ul></details>{/if}
        {#if importStatus.syncError}<p class="error" role="alert">{importStatus.syncError}</p><button onclick={refreshImported}>목록 새로고침</button>
        {:else if importStatus.result.imported}<button onclick={()=>{closeTools();void showImported();}}>캔버스에서 보기</button>{/if}
       {/if}
      </div>{/if}
      <details class="import-scope" use:disclosure><summary>조회 폴더</summary><small>BiBi 실행 호스트 · 현재 프로젝트 폴더</small><code>{project.workspace}</code></details>
      {/if}
     </div>{/if}
    </div>
    <button class="primary new-work" onclick={()=>showModal(project?'new':'project')}><Icon name="plus" /><span>새 업무</span></button>
   {:else if view==='conversation'&&run}
    <button class="icon-button" aria-label="업무 맥락" aria-pressed={contextOpen} title="업무 맥락" onclick={()=>{freezeOffscreenMessages(messagesPane);contextOpen=!contextOpen;}}><Icon name="context" /></button>
   {:else if view==='usage'}<button disabled={refreshingUsage} aria-busy={refreshingUsage} onclick={refreshUsage}>{refreshingUsage?'갱신 중…':'새로고침'}</button>{/if}
  </div>{/if}
  {#if windowsWindow}<WindowControls onerror={message=>error=message} />{/if}
 </header>
 {#if view==='usage'&&usageError}<div class="global-error" role="alert">{usageError}</div>{/if}
 {#if error}<div class="global-error" role="alert" use:scrollbars transition:reveal><span>{error}</span><button aria-label="오류 닫기" class="icon-button" onclick={()=>error=''}><Icon name="close" /></button></div>{/if}
 {#if needsAuth}
 <main class="login-screen" use:scrollbars><form class="card login-card" onsubmit={(e)=>{e.preventDefault();void authenticate();}}><h1>서버 연결</h1><label>인증 토큰<input type="password" autocomplete="off" bind:value={token} required /></label><button class="primary">연결</button></form></main>
 {:else if !snapshot}
 <main class="login-screen" use:scrollbars><div class="card login-card"><p>{loading?'연결 중…':'서버 연결 끊김'}</p><button onclick={connect} disabled={loading}>다시 연결</button></div></main>
 {:else}
 {#key view}
 <main class={'main-content '+view} bind:clientHeight={contentHeight} use:scrollbars in:surfaceFade aria-label={view==='usage'?'사용량과 연결':view==='conversation'?'대화 영역':'캔버스 영역'}>
  {#if view==='canvas'}
   <div class="canvas-layout" class:has-selection={!!run} class:stacked-inspector={inspectorStacked}>
    <Canvas bind:this={canvas} runs={sessions} {works} {memberships} edges={canvasEdges} selected={sessions.find(r=>sessionId(r)===sessionId(run))?.id??selected} storageKey={'bibi:board:'+snapshot.server_id+':'+projectId} {focusedWork} onfocus={id=>navigate({group:id,run:''})} onselect={select} onopen={open} {halfLife} {floor} uiScale={canvasScale} />
    <div class="canvas-inspector" inert={!run} aria-hidden={!run}>
     {#if run}<PanelResize label={inspectorStacked?'세션 상세 높이':'세션 상세 너비'} value={inspectorStacked?inspectorHeight:inspectorWidth} min={inspectorStacked?6:18} max={inspectorStacked?detailHeightMax:detailMax} unit={fontSize} axis={inspectorStacked?'y':'x'} direction={-1} onresize={v=>resizePanel(inspectorStacked?'inspectorHeight':'inspector',v)} onactive={v=>resizing=v} oncommit={savePanels} onreset={()=>resetPanel(inspectorStacked?'inspectorHeight':'inspector')} />{/if}
     {#if run}<RunDetails {run} {work} {works} {memberships} groupEditing={snapshot.session_groups_v1===true} host={snapshot.hosts.find(h=>h.id===run.host_id)} {now} onopen={()=>open(run.id)} onclose={()=>navigate({run:''})} onchanged={refreshSnapshot} />{/if}
    </div>
   </div>
  {:else if view==='conversation'}
   {#if run&&project}
    <div class="conversation-layout" class:has-context={contextOpen}>
     <section class="conversation-panel card">
      <div class="conversation-heading panel-header" use:scrollbars aria-label="세션 정보"><div><strong>{run.title}</strong><small title={[providerName(run,snapshot.providers),run.model||'모델 확인 대기',sessionId(run),run.host_id].join(' · ')}>{providerName(run,snapshot.providers)} · {run.model||'모델 확인 대기'} · {shortId(sessionId(run))} · {run.host_id}</small></div><span class={'badge '+run.state} title={run.wait_reason??(run.state==='queued'?'BiBi에 저장됨 · 제공자에 아직 전송하지 않음':run.phase==='응답 대기'?'제공자에 전달됨 · 응답을 기다리는 중':run.phase)}>{stateLabel(run)}</span><SessionActions {run} {works} {memberships} groupEditing={snapshot.session_groups_v1===true} onchanged={refreshSnapshot}>
       {#if (isActive(run.state)||run.state==='queued')&&run.capabilities.interrupt.supported}<button class="danger-button" onclick={()=>action({type:'interrupt',run_id:run.id})} disabled={run.phase==='중단 요청 중'}>{run.phase==='중단 요청 중'?'중단 중…':'중단'}</button>{/if}</SessionActions>
      </div>
      <div bind:this={messagesPane} class="messages" use:scrollbars aria-label="대화 기록" aria-live="polite" use:conversationScroll={{following:()=>followTail,set:value=>followTail=value}}>
       {#if detail?.run.id===run.id}
        {#each (detail.conversation??detail.messages).filter(message=>message.text.length>0) as message(message.id)}
         {#if message.role==='assistant'&&message.phase==='commentary'}
          <article class="message commentary"><details use:disclosure><summary><span>진행 안내</span><time>{new Date(message.created_at).toLocaleTimeString('ko-KR',{hour:'2-digit',minute:'2-digit'})}</time></summary><Markdown text={message.text} /></details></article>
         {:else}<article class={'message '+message.role}>
          <div class="message-meta"><span>{message.role==='user'?'사용자':message.role==='assistant'?run.role:message.role==='tool'?'도구':'시스템'}{message.id.startsWith('input:')?' · 전달됨':''}</span><time>{new Date(message.created_at).toLocaleTimeString('ko-KR',{hour:'2-digit',minute:'2-digit'})}</time></div>
          {#if message.role==='tool'}<details use:disclosure><summary>{({'bibi_consult':'전문가 문의','bibi_inbox':'회신 확인','bibi_report':'진행 보고','bibi_guild_read':'길드 조회','bibi_guild_record':'길드 기록','consult':'전문가 문의','inbox':'회신 확인','report':'진행 보고','guild_read':'길드 조회','guild_record':'길드 기록'} as Record<string,string>)[message.text.split('\n')[0]]??'실행 기록'}</summary><div class="message-text">{message.text}</div></details>
          {:else if message.role==='assistant'}<Markdown text={message.text} />{:else}<div class="message-text">{message.text}</div>{/if}
         </article>{/if}
        {/each}
        {#each detail.approvals.filter(a=>a.state==='pending') as approval(approval.id)}<ApprovalForm {approval} onrespond={async(id,value)=>{await command({type:'respond',approval_id:id,value});await loadDetail(run.id);}} />{/each}
        {#each (detail.inputs??[]).filter(i=>i.state!=='delivered') as input(input.id)}<p class="input-status"><span class="badge">{{accepted:'접수됨',sending:'전달 확인 중',delivered:'전달됨',failed:'전달 실패',uncertain:'확인 필요'}[input.state]??input.state}</span> {input.text}</p>{/each}
        {#if run.state==='queued'&&run.wait_reason}<p class="input-status" role="status">{run.wait_reason}</p>{/if}
      {#if run.error}<p class="error">{run.error}</p>{/if}
        {#if run.state==='uncertain'&&run.origin==='managed'&&run.capabilities.continue_session?.supported}<label class="check recovery"><input type="checkbox" onchange={(e)=>{if(e.currentTarget.checked)void action({type:'resolve_run',run_id:run.id,confirmed_stopped:true});}} />이전 프로세스가 종료됐고 파일 변경을 확인한 경우에만 체크하세요.</label>{/if}
        {#if run.origin==='external'&&run.state==='uncertain'}<p class="input-status" role="status">{run.phase}</p>{/if}
        {#if run.activity}<p class="activity-line">● {run.activity.summary} · 보고 {age(run.activity.reported_at,now)}</p>{/if}
       {:else}<p class="muted">기록 불러오는 중…</p>{/if}
      </div>
      <Composer bind:this={conversationComposer} externalResume={snapshot.external_resume_v1===true} onresumed={refreshSnapshot} serverId={snapshot.server_id} serverApprovals={snapshot.approval_modes_v1===true} {project} {run} work={work??null} hosts={snapshot.hosts} providers={snapshot.providers} modelHistory={snapshot.model_history} selection={snapshot.model_selection} onsettings={()=>showModal('settings')} onlocal={localCommand} onaccepted={accepted} />
     </section>
     <div class="conversation-inspector" inert={!contextOpen} aria-hidden={!contextOpen}>
     {#if contextOpen}<PanelResize label={contextStacked?'업무 맥락 높이':'업무 맥락 너비'} value={contextStacked?contextHeight:contextWidth} min={contextStacked?6:18} max={contextStacked?detailHeightMax:detailMax} unit={fontSize} axis={contextStacked?'y':'x'} direction={-1} onresize={v=>resizePanel(contextStacked?'contextHeight':'context',v)} onactive={v=>resizing=v} oncommit={savePanels} onreset={()=>resetPanel(contextStacked?'contextHeight':'context')} />{/if}
     <aside class="context-panel card" aria-label="업무 맥락 내용"><div class="row panel-header"><strong>맥락</strong><div class="row"><button onclick={editContext}>편집</button><button class="icon-button" aria-label="맥락 닫기" onclick={()=>contextOpen=false}><Icon name="close" /></button></div></div><div class="panel-body" use:scrollbars aria-label="맥락 기록"><small>업무 v{work?.context_revision} · 실행 v{run.context_revision}</small>
      <h3 class="context-label">목표</h3><p>{work?.goal??run.context.goal}</p><h3 class="context-label">제약</h3><ul>{#each work?.constraints??run.context.constraints as constraint}<li>{constraint}</li>{/each}</ul>
      {#if work?.decisions.length}<h3 class="context-label">결정</h3>{#each work.decisions as d}<p>{d.text}<small>{d.source} · {d.revision}</small></p>{/each}{/if}
      {#if work?.performed_actions.length}<h3 class="context-label">이미 적용한 변경</h3>{#each work.performed_actions as d}<p>{d.text}<small>{d.source}</small></p>{/each}{/if}
      {#if run.context.references.length}<h3 class="context-label">참조 자료</h3>{#each run.context.references as reference}<p class="reference">{reference.text}<small>{reference.source}</small></p>{/each}{/if}
      {#if run.context.previous_answer_excerpt}<details use:disclosure><summary>이전 답변 발췌{run.context.excerpt_truncated?' · 일부':''}</summary><p class="prewrap">{run.context.previous_answer_excerpt}</p></details>{/if}
      <details use:disclosure><summary>실행 사용량</summary><dl><dt>입력 토큰</dt><dd>{run.stats.input_tokens??'확인 불가'}</dd><dt>캐시 입력</dt><dd>{run.stats.cached_input_tokens??'확인 불가'}</dd><dt>출력 토큰</dt><dd>{run.stats.output_tokens??'확인 불가'}</dd></dl></details>
      {#if detail?.inbox.length}<details use:disclosure><summary>수신함 · {detail.inbox.length}</summary>{#each detail.inbox as entry}<button class="inbox-item" onclick={()=>open(entry.from_run_id)}>{shortId(entry.from_run_id)} · {age(entry.created_at,now)}{entry.late?' · 늦은 결과':''}{entry.context_revision!==work?.context_revision?' · 이전 맥락':''}</button>{/each}</details>{/if}
     </div></aside></div>
    </div>
   {:else}<div class="empty-state"><p>선택한 실행 없음</p><button onclick={()=>navigate({view:'canvas'})}>캔버스 열기</button><button class="primary" onclick={()=>showModal(project?'new':'project')}>새 업무</button></div>{/if}
  {:else}
   <QuotaCards {quotas} {now} connections={snapshot.providers} expanded />
   <div class="card host-panel"><div class="row"><h2>호스트</h2><button disabled={!project} onclick={()=>showModal('host')}>호스트 연결</button></div>{#each snapshot.hosts as host}<div class="host-row"><div><strong>{host.name}</strong><small>{host.platform} · {snapshot.providers.filter(p=>p.host_id===host.id).map(p=>p.name).join(' · ')||'등록된 제공자 없음'}</small></div><span class={'badge '+(host.connected&&now-host.observed_at<30000?'connected':'warn')}>{host.connected&&now-host.observed_at<30000?'연결됨':'확인 필요'}</span><small>{age(host.observed_at,now)}</small>{#if host.error}<p class="error">{host.error}</p>{/if}</div>{/each}</div>
   <div class="card host-panel"><h2>연결 범위</h2><dl><dt>서버 주소</dt><dd>{endpoint}</dd><dt>서버</dt><dd>{snapshot.server_id}</dd><dt>실행</dt><dd>BiBi 실행 {snapshot.runs.filter(r=>r.origin==='managed').length} · 외부 {snapshot.runs.filter(r=>r.origin==='external').length}</dd><dt>관측 기준</dt><dd>이 서버에 연결된 호스트와 등록된 세션</dd></dl></div>
  {/if}
 </main>
 {/key}
 {/if}
 </div>
</div>

{#if sidebarDrawer&&compactNavigation}
 <dialog class="sidebar-drawer" class:mac-window={macWindow} class:windows-window={windowsWindow} use:modalDialog={{returnFocus:()=>sidebarToggle}} transition:drawerReveal aria-label="사이드바" oncancel={(e)=>{e.preventDefault();void closeSidebar();}} onclick={(e)=>{if(e.target===e.currentTarget){const box=e.currentTarget.getBoundingClientRect();if(e.clientX<box.left||e.clientX>box.right||e.clientY<box.top||e.clientY>box.bottom)void closeSidebar();}}}>
  {@render sidebar()}
  {#if windowsWindow}<WindowControls onerror={message=>error=message} />{/if}
 </dialog>
{/if}

{#if modal}
<div class="modal-backdrop" class:windows-window={windowsWindow} role="presentation" onclick={(e)=>{if(e.target===e.currentTarget)closeModal();}}>
 <dialog class="modal card" use:modalDialog use:scrollbars transition:surfaceFade oncancel={(e)=>{e.preventDefault();closeModal();}} aria-label={modal==='project'?'프로젝트 추가':modal==='new'?'새 업무':modal==='context'?'업무 맥락':modal==='host'?'호스트 연결':modal==='search'?'세션 검색':modal==='cleanup'?'연결 끊긴 세션 정리':modal==='extensions'?'프로젝트 확장':'설정'} tabindex="-1">
  <div class="row"><h2>{modal==='project'?'프로젝트 추가':modal==='new'?'새 업무':modal==='context'?'업무 맥락':modal==='host'?'호스트 연결':modal==='search'?'세션 검색':modal==='cleanup'?'연결 끊긴 세션 정리':modal==='extensions'?'프로젝트 확장':'설정'}</h2><button class="icon-button" aria-label="닫기" onclick={closeModal}><Icon name="close" /></button></div>
  {#if modal==='project'}<form onsubmit={(e)=>{e.preventDefault();void createProject();}}><label>프로젝트 이름<input bind:value={name} required /></label><label>호스트 작업 경로<input bind:value={workspace} required placeholder="/path/to/project" /></label><label>openguild 경로<input bind:value={guild} placeholder="선택" /></label><div class="form-actions"><button class="primary">추가</button></div></form>
  {:else if modal==='host'}<form onsubmit={(e)=>{e.preventDefault();void registerHost();}}>
   <label>호스트 이름<input bind:value={hostName} required /></label><label>서버 주소<input type="url" bind:value={hostUrl} placeholder="https://host.example" required /></label>
   <label>호스트 인증 토큰<input type="password" autocomplete="off" bind:value={hostToken} required /></label><label>이 프로젝트의 호스트 작업 경로<input bind:value={hostWorkspace} required /></label>
   <label>호스트의 openguild 경로<input bind:value={hostGuild} placeholder="선택" /></label>{#if error}<p class="error" role="alert">{error}</p>{/if}<div class="form-actions"><button class="primary" disabled={savingHost}>{savingHost?'연결 중…':'연결'}</button></div>
  </form>
  {:else if modal==='new'&&snapshot&&project}<Composer serverId={snapshot.server_id} serverApprovals={snapshot.approval_modes_v1===true} {project} hosts={snapshot.hosts} providers={snapshot.providers} modelHistory={snapshot.model_history} selection={snapshot.model_selection} onsettings={()=>showModal('settings')} onlocal={localCommand} onaccepted={accepted} />
  {:else if modal==='context'&&work}<form onsubmit={(e)=>{e.preventDefault();void saveContext();}}><label>목표<textarea use:scrollbars bind:value={contextGoal} required rows="4"></textarea></label><label>제약 · 한 줄에 하나<textarea use:scrollbars bind:value={contextConstraints} rows="5"></textarea></label><div class="form-actions"><button class="primary">저장</button></div></form>
  {:else if modal==='search'&&snapshot&&project}<SessionSearch externalResume={snapshot.external_resume_v1===true} {project} initialQuery={searchQuery} providers={snapshot.providers} onopen={next=>{if(snapshot)mergeRun(snapshot.runs,next);navigate({run:next.id,view:'conversation',modal:'',group:''});}} />
  {:else if modal==='cleanup'&&snapshot&&project}{#key snapshot.server_id+':'+project.id}<SessionCleanup {snapshot} {project} onchanged={refreshSnapshot} onclose={closeModal} onrestore={showRemoved} />{/key}
  {:else if modal==='extensions'&&snapshot&&project}{#key project.id}<ProjectExtensions {project} providers={snapshot.providers} hosts={snapshot.hosts} />{/key}
  {:else if modal==='settings'}<ThemeSettings /><ProviderSettings providers={snapshot?.providers??[]} onchanged={refreshSnapshot} /><div class="scale-setting"><div class="row"><label for="ui-scale">UI 크기 · {Math.round(uiScale*100)}%</label><button onclick={()=>{uiScale=1;appearance();}}>100%로 복원</button></div><input id="ui-scale" type="range" min=".5" max="2" step=".05" bind:value={uiScale} oninput={appearance} /></div><div class="form-grid"><label>화살표 반감기 · 초<input type="number" min="1" max="3600" bind:value={halfLife} onchange={appearance} /></label><label>최소 불투명도<input type="range" min=".05" max=".5" step=".05" bind:value={floor} onchange={appearance} /></label></div>
   <details bind:this={removedSection} class="removed-sessions" use:disclosure><summary>제거한 세션 · {sessionNodes(snapshot?.removed_sessions??[]).length}</summary>{#each sessionNodes(snapshot?.removed_sessions??[]) as removed}<div class="row removed-session"><span>{removed.title}</span><button onclick={async()=>{await action({type:'set_session_hidden',run_id:removed.id,hidden:false});await refreshSnapshot();}}>복원</button></div>{:else}<p class="muted">제거한 세션이 없습니다.</p>{/each}</details>
   {#if isDesktop()}<form onsubmit={(e)=>{e.preventDefault();void changeConnection();}}><label>서버 주소<input type="url" bind:value={remoteUrl} placeholder="비워 두면 로컬" /></label><label>인증 토큰<input type="password" bind:value={remoteToken} autocomplete="off" /></label><div class="form-actions"><button class="primary">서버 변경</button></div></form>{/if}
   <small class="endpoint">{endpoint}</small>{#if snapshot&&!isDesktop()}<button onclick={async()=>{await request('/auth/logout','POST');modal='';snapshot=null;await connect();}}>연결 해제</button>{/if}
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
  {#if windowsWindow}<WindowControls onerror={message=>error=message} />{/if}
 </dialog>
</div>
{/if}
