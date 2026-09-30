<script lang="ts">
import Icon from './Icon.svelte';
import { onMount, untrack } from 'svelte';
import type {Run,Work,Transmission} from '../types';
import { providerName,stateLabel,shortId } from '../format';
import {sessionId} from '../sessions';
import { type Point,type Viewport,type CameraFrame,resizeViewport,arrangeNodes,latestConnections,zoomAt,wheelZoomFactor,fit,opacity,edgePath,nodeSize,translateGroup,scalePoints,dragDelta,NODE_WIDTH,NODE_HEIGHT } from '../board';
let {runs,works,edges,selected,storageKey,focusedWork='',onfocus,onselect,onopen,halfLife=30,floor=.15,uiScale=1}: {
 runs:Run[];works:Work[];edges:Transmission[];selected:string;storageKey:string;focusedWork?:string;onfocus:(id:string)=>void;
 onselect:(id:string)=>void;onopen:(id:string)=>void;halfLife?:number;floor?:number;uiScale?:number;
} = $props();
let root:HTMLDivElement,toolbar:HTMLDivElement;
let points=$state<Record<string,Point>>({});
let view=$state<Viewport>({pan:{x:20,y:40},zoom:1});
let width=$state(800),height=$state(550),now=$state(Date.now()),ready=$state(false);
let loadedKey='',loadedFocus='';
let cameras:Record<string,CameraFrame>={};
let saveTimer:ReturnType<typeof setTimeout>|undefined;
let frame=0;
let reframing=$state(false),motionTimer:ReturnType<typeof setTimeout>|undefined;
// Commit the destination once; animate a shared visual frame for nodes, paths,
// groups and camera. A reload during motion therefore restores the destination.
let arrangement=$state<{points:Record<string,Point>;view:Viewport}|null>(null);
let arrangementFrame=0,reducedMotion=false;
const renderedView=$derived(arrangement?.view??view);
const scoped=$derived(focusedWork?runs.filter(r=>r.work_id===focusedWork):runs);
const sizes=$derived(new Map(runs.map(r=>[sessionId(r),nodeSize(r.agent_kind,uiScale)])));
const members=$derived.by(()=>{const map=new Map<string,string[]>();for(const r of scoped){const ids=map.get(r.work_id)??[];ids.push(sessionId(r));map.set(r.work_id,ids);}return map;});
let camera:CameraFrame|null=null;
let pointers=new Map<number,Point>();
let drag:{id:number;node:string|null;group:string|null;last:Point;start:Point;moved:boolean}|null=null;
let pinchDistance=0;
let lastDrag=0;
function validCamera(value:unknown):value is CameraFrame{
 const c=value as CameraFrame|undefined;return !!c&&[c.size?.width,c.size?.height,c.view?.zoom,c.view?.pan?.x,c.view?.pan?.y].every(Number.isFinite)&&c.size.width>0&&c.size.height>0&&c.view.zoom>0;
}
function flush(){
 clearTimeout(saveTimer);saveTimer=undefined;if(!loadedKey)return;
 if(camera)cameras[loadedFocus]=camera;
 try{localStorage.setItem(loadedKey,JSON.stringify({points,view:cameras['']?.view??view,selected,camera:cameras['']??camera,cameras}));}catch{/* Storage may be disabled. */}
}
function persist(){clearTimeout(saveTimer);saveTimer=setTimeout(flush,180);}
function stopArrangement(keepVisible=false){
 cancelAnimationFrame(arrangementFrame);arrangementFrame=0;
 if(keepVisible&&arrangement){
  points={...points,...arrangement.points};view=arrangement.view;
  camera={view:{zoom:view.zoom,pan:{...view.pan}},size:{width,height}};
  persist();
 }
 arrangement=null;
}
function animate(){reframing=true;clearTimeout(motionTimer);motionTimer=setTimeout(()=>reframing=false,280);}
$effect(()=>{
 const key=storageKey,scope=focusedWork,all=runs;
 untrack(()=>{
  if(key!==loadedKey||scope!==loadedFocus)stopArrangement();
  if(key!==loadedKey){
   flush();loadedKey=key;loadedFocus='';points={};view={pan:{x:20,y:40},zoom:1};camera=null;cameras={};
   try {const saved=JSON.parse(localStorage.getItem(key)??'null');if(saved){points=saved.points??{};view=saved.view??view;camera=validCamera(saved.camera)?saved.camera:null;cameras=Object.fromEntries(Object.entries(saved.cameras??{}).filter(([,v])=>validCamera(v))) as Record<string,CameraFrame>;if(camera)cameras['']=camera;}}catch{/* New layout. */}
  }
  const arranged=arrangeNodes(all),next={...points};let changed=false;
  for(const [id,p] of Object.entries(arranged))if(!next[id]){next[id]=p;changed=true;}
  if(changed)points=next;
  if(root){
   width=root.clientWidth;height=root.clientHeight;
   if(scope!==loadedFocus){
    if(camera)cameras[loadedFocus]=camera;
    loadedFocus=scope;camera=cameras[scope]??null;animate();
    if(!camera){view=fitScope();rebaseCamera();}
   }
   if(camera)view=resizeViewport(camera,{width,height});else rebaseCamera();
   persist();
  }
 });
});
onMount(()=>{
 ready=true;
 const media=window.matchMedia('(prefers-reduced-motion: reduce)');
 const motionPreference=()=>{reducedMotion=media.matches;if(reducedMotion)stopArrangement();};
 motionPreference();media.addEventListener('change',motionPreference);
 const observer=new ResizeObserver(()=>{
  const size={width:root.clientWidth,height:root.clientHeight};
  if(!size.width||!size.height)return;
  if(camera)view=resizeViewport(camera,size);
  width=size.width;height=size.height;
  if(!camera)rebaseCamera();
 });
 observer.observe(root);
 const timer=setInterval(()=>{if(document.visibilityState==='visible')now=Date.now();},500);
 window.addEventListener('pagehide',flush);
 return()=>{flush();stopArrangement();media.removeEventListener('change',motionPreference);observer.disconnect();clearInterval(timer);clearTimeout(motionTimer);cancelAnimationFrame(frame);window.removeEventListener('pagehide',flush);};
});
const displayPoints=$derived(scalePoints(arrangement?.points??points,uiScale));
const visible=$derived(scoped.filter(r=>{const p=displayPoints[sessionId(r)],size=nodeSize(r.agent_kind,uiScale);return p && (p.x+size.width)*renderedView.zoom+renderedView.pan.x>=-80 && p.x*renderedView.zoom+renderedView.pan.x<=width+80 && (p.y+size.height)*renderedView.zoom+renderedView.pan.y>=-80 && p.y*renderedView.zoom+renderedView.pan.y<=height+80;}));
const connections=$derived(latestConnections(edges));
const paths=$derived(connections.flatMap(e=>{
 if(!e.from_run_id||!sizes.has(e.from_run_id)||!sizes.has(e.to_run_id))return [];
 const a=displayPoints[e.from_run_id],b=displayPoints[e.to_run_id];
 if(!a||!b)return [];
 return [{...e,path:edgePath(a,b,e.kind==='reply',sizes.get(e.from_run_id),sizes.get(e.to_run_id)),a,b}];
}));
const scopedIds=$derived(new Set(scoped.map(sessionId)));
function inView(a:Point,b:Point){return (Math.max(a.x,b.x)+NODE_WIDTH*uiScale)*renderedView.zoom+renderedView.pan.x>=0&&Math.min(a.x,b.x)*renderedView.zoom+renderedView.pan.x<=width&&(Math.max(a.y,b.y)+NODE_HEIGHT*uiScale)*renderedView.zoom+renderedView.pan.y>=0&&Math.min(a.y,b.y)*renderedView.zoom+renderedView.pan.y<=height;}
const visibleEdges=$derived(paths.filter(e=>scopedIds.has(e.from_run_id!)&&scopedIds.has(e.to_run_id)&&inView(e.a,e.b)));
const parentPaths=$derived(scoped.flatMap(r=>{
 const parent=r.parent_session_id,a=parent?displayPoints[parent]:null,b=displayPoints[sessionId(r)];
 return a&&b&&parent&&scopedIds.has(parent)?[{id:r.id,a,b,path:edgePath(a,b,false,sizes.get(parent),sizes.get(sessionId(r)))}]:[];
}));
const visibleParents=$derived(parentPaths.filter(p=>inView(p.a,p.b)));
const groups=$derived(works.flatMap(w=>{
 const ps=(members.get(w.id)??[]).flatMap(id=>displayPoints[id]?[{...displayPoints[id],...sizes.get(id)!}]:[]);
 if(!ps.length)return [];
 const x=Math.min(...ps.map(p=>p.x))-16*uiScale,y=Math.min(...ps.map(p=>p.y))-38*uiScale;
 return [{work:w,x,y,w:Math.max(...ps.map(p=>p.x+p.width))+16*uiScale-x,h:Math.max(...ps.map(p=>p.y+p.height))+16*uiScale-y}];
}));
function rebaseCamera(){
 // A toolbar action can run before ResizeObserver delivers the new layout.
 // Capture the dimensions used by the visible viewport, not the prior frame.
 width=root.clientWidth;height=root.clientHeight;
 camera={view:{zoom:view.zoom,pan:{...view.pan}},size:{width,height}};
}
function local(event:PointerEvent|WheelEvent):Point{const box=root.getBoundingClientRect();return{x:event.clientX-box.left,y:event.clientY-box.top};}
function down(event:PointerEvent,node:string|null=null,group:string|null=null){
 if(event.button!==0)return;
 stopArrangement(true);
 event.stopPropagation();const p=local(event);pointers.set(event.pointerId,p);
 (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
 if(pointers.size===2){drag=null;const p=[...pointers.values()];pinchDistance=Math.hypot(p[0].x-p[1].x,p[0].y-p[1].y);return;}
 drag={id:event.pointerId,node,group,last:p,start:p,moved:false};
}
function move(event:PointerEvent){
 if(!pointers.has(event.pointerId))return;
 const current=local(event),previous=pointers.get(event.pointerId)!;pointers.set(event.pointerId,current);
 if(pointers.size===2){
  const p=[...pointers.values()],distance=Math.hypot(p[0].x-p[1].x,p[0].y-p[1].y);
  if(pinchDistance>0)view=zoomAt(view,{x:(p[0].x+p[1].x)/2,y:(p[0].y+p[1].y)/2},view.zoom*distance/pinchDistance);
  rebaseCamera();pinchDistance=distance;lastDrag=Date.now();return;
 }
 if(!drag||drag.id!==event.pointerId)return;
 const dx=current.x-previous.x,dy=current.y-previous.y;
 if(Math.hypot(current.x-drag.start.x,current.y-drag.start.y)>4)drag.moved=true;
 if(!drag.moved)return;
 if(drag.node)points=translateGroup(points,[drag.node],dragDelta({x:dx,y:dy},view.zoom,uiScale));
 else if(drag.group)points=translateGroup(points,members.get(drag.group)??[],dragDelta({x:dx,y:dy},view.zoom,uiScale));
 else {view={...view,pan:{x:view.pan.x+dx,y:view.pan.y+dy}};rebaseCamera();}
 drag.last=current;
}
function up(event:PointerEvent){pointers.delete(event.pointerId);if(drag?.moved)lastDrag=Date.now();drag=null;pinchDistance=0;flush();}
let wheels:{point:Point;factor:number;dx:number;dy:number}[]=[];
function wheel(event:WheelEvent){
 stopArrangement(true);
 event.preventDefault();wheels.push({point:local(event),factor:event.ctrlKey||event.metaKey?wheelZoomFactor(event.deltaY,event.deltaMode,navigator.platform):0,dx:event.deltaX,dy:event.deltaY});
 if(!frame)frame=requestAnimationFrame(()=>{frame=0;for(const w of wheels)view=w.factor?zoomAt(view,w.point,view.zoom*w.factor):{...view,pan:{x:view.pan.x-w.dx,y:view.pan.y-w.dy}};wheels=[];rebaseCamera();persist();});
}
function scale(factor:number){stopArrangement(true);view=zoomAt(view,{x:width/2,y:height/2},view.zoom*factor);rebaseCamera();persist();}
function fitScope(){
 const ps=scoped.flatMap(r=>{const p=points[sessionId(r)];return p?[{x:(p.x-16)*uiScale,y:(p.y-38)*uiScale,width:(nodeSize(r.agent_kind).width+32)*uiScale,height:(nodeSize(r.agent_kind).height+54)*uiScale}]:[];});
 return fit(ps,root.clientWidth,Math.max(1,root.clientHeight-(toolbar?.offsetHeight??35)-28*uiScale));
}
export function fitAll(){stopArrangement(true);view=fitScope();rebaseCamera();persist();}
function arrange(){
 stopArrangement(true);
 const fromPoints=points,fromView=view;
 const next=arrangeNodes(scoped);
 if(focusedWork){
  const old=scoped.map(r=>points[sessionId(r)]).filter(Boolean),ps=Object.values(next);
  if(old.length&&ps.length){const dx=Math.min(...old.map(p=>p.x))-Math.min(...ps.map(p=>p.x)),dy=Math.min(...old.map(p=>p.y))-Math.min(...ps.map(p=>p.y));for(const p of ps){p.x+=dx;p.y+=dy;}}
 }
 points={...points,...next};view=fitScope();rebaseCamera();persist();
 if(reducedMotion)return;
 reframing=false;clearTimeout(motionTimer);
 const targetPoints=points,start=performance.now();
 const fromCamera:CameraFrame={view:fromView,size:{width,height}};
 const targetCamera:CameraFrame={view,size:{width,height}};
 arrangement={points:fromPoints,view:fromView};
 function step(time:number){
  const progress=Math.min(1,(time-start)/260),ease=1-Math.pow(1-progress,3);
  if(progress>=1){stopArrangement();return;}
  const mix=(a:number,b:number)=>a+(b-a)*ease;
  // Sidebar/window resizing changes the viewport, not the chosen destination.
  // Reframe both endpoints together instead of saving an interrupted layout.
  const from=resizeViewport(fromCamera,{width,height}),to=resizeViewport(targetCamera,{width,height});
  arrangement={
   points:Object.fromEntries(Object.entries(targetPoints).map(([id,p])=>{const a=fromPoints[id]??p;return[id,{x:mix(a.x,p.x),y:mix(a.y,p.y)}];})),
   view:{zoom:mix(from.zoom,to.zoom),pan:{x:mix(from.pan.x,to.pan.x),y:mix(from.pan.y,to.pan.y)}}
  };
  arrangementFrame=requestAnimationFrame(step);
 }
 arrangementFrame=requestAnimationFrame(step);
}
function shiftGroup(id:string,delta:Point){stopArrangement(true);points=translateGroup(points,members.get(id)??[],delta);persist();}

function focus(id:string){if(Date.now()-lastDrag<200)return;flush();onfocus(id);}
function select(id:string){if(Date.now()-lastDrag<200)return;onselect(id);persist();}
</script>
<div class="board-wrap" class:reframing>
 <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (The canvas has keyboard zoom and focusable node buttons.) -->
 <div bind:this={root} class="board" role="application" aria-label="세션 캔버스" tabindex="0"
  onpointerdown={(e)=>down(e)} onpointermove={move} onpointerup={up} onpointercancel={up} onwheel={wheel}
  onkeydown={(e)=>{if(e.target!==root)return;if(e.key==='+')scale(1.25);else if(e.key==='-')scale(.8);else if(e.key==='0')fitAll();}}>
  {#if ready}
  <div class="world" style:transform={'translate('+renderedView.pan.x+'px,'+renderedView.pan.y+'px) scale('+renderedView.zoom+')'}>
   {#each groups as group(group.work.id)}
    <div role="button" tabindex="0" aria-label={group.work.title+' 그룹 이동'} data-work-id={group.work.id} ondblclick={()=>focus(group.work.id)} onpointerdown={(e)=>down(e,null,group.work.id)} onkeydown={(e)=>{if(e.key==='Enter'){e.preventDefault();focus(group.work.id);return;}const d=({ArrowLeft:{x:-20,y:0},ArrowRight:{x:20,y:0},ArrowUp:{x:0,y:-20},ArrowDown:{x:0,y:20}} as Record<string,Point>)[e.key];if(d){e.preventDefault();shiftGroup(group.work.id,d);}}} class="work-group" style:left={group.x+'px'} style:top={group.y+'px'} style:width={group.w+'px'} style:height={group.h+'px'}>
     <span>{shortId(group.work.id)} · {group.work.title}</span>
    </div>
   {/each}
   <svg class="connections" aria-hidden="true">
    <defs><marker id="arrow" markerWidth="9" markerHeight="7" refX="8" refY="3.5" orient="auto"><path d="M 0 0 L 9 3.5 L 0 7 z" fill="currentColor" /></marker></defs>
    {#each visibleParents as child(child.id)}
     <path d={child.path} stroke="currentColor" fill="none" stroke-width="1" stroke-dasharray="4 5" opacity=".25" />
    {/each}
    {#each visibleEdges as edge(edge.id)}
     <g class:reply={edge.kind==='reply'} style:opacity={opacity(edge.sent_at,now,halfLife,floor)}>
      <path d={edge.path} stroke="currentColor" fill="none" stroke-width="2" marker-end="url(#arrow)" />
     </g>
    {/each}
   </svg>
   {#each visible as run(run.id)}
    <button data-session-id={sessionId(run)} class="run-node" class:subagent={run.agent_kind==='subagent'} style:width={nodeSize(run.agent_kind,uiScale).width+'px'} style:height={nodeSize(run.agent_kind,uiScale).height+'px'} class:selected={selected===run.id} class:bad={['failed','uncertain','disconnected'].includes(run.state)}
     style:left={displayPoints[sessionId(run)].x+'px'} style:top={displayPoints[sessionId(run)].y+'px'} aria-pressed={selected===run.id}
     onpointerdown={(e)=>down(e,sessionId(run))} onclick={()=>select(run.id)} ondblclick={()=>{if(Date.now()-lastDrag>=200)onopen(run.id);}}
     onkeydown={(e)=>{if(e.key==='Enter'){e.preventDefault();onopen(run.id);}}}>
     <span class="node-meta">{run.agent_kind==='subagent'?'서브에이전트':run.role} · {providerName(run)}{run.origin==='external'&&run.agent_kind!=='subagent'?' · 외부':''}</span>
     <strong>{run.title}</strong>
     {#if run.agent_kind!=='subagent'}<span class="node-model">{run.model||'모델 확인 대기'}</span>{/if}
     <span class="node-bottom"><span class={'status-text '+run.state}>● {stateLabel(run)}</span><span>{shortId(sessionId(run))}</span></span>
    </button>
   {/each}
  </div>
  {/if}
  {#if !runs.length}<div class="empty-board">등록된 세션 없음</div>{/if}
 </div>
 {#if focusedWork}<div class="board-back"><button onclick={()=>focus('')}><Icon name="back" />전체 그룹</button><span>{works.find(w=>w.id===focusedWork)?.title}</span></div>{/if}
 <div bind:this={toolbar} class="board-tools" role="group" aria-label="캔버스 보기">
  <details class="board-help"><summary aria-label="캔버스 도움말" title="캔버스 도움말"><Icon name="help" /></summary><div class="board-legend card"><span><i></i>최근 전송</span><span><i class="faded"></i>시간 경과</span><span>┄ 부모 연결</span><span>더블클릭 · 대화 열기</span></div></details>
  <button class="icon-button" aria-label="축소" onclick={()=>scale(.8)}><Icon name="minus" /></button>
  <span>{Math.round(renderedView.zoom*100)}%</span>
  <button class="icon-button" aria-label="확대" onclick={()=>scale(1.25)}><Icon name="plus" /></button>
  <button onclick={arrange}>자동 정렬</button>
  <button onclick={fitAll}>전체 보기</button>
 </div>
</div>
