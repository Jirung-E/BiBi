<script lang="ts">
import { onMount, type Snippet } from 'svelte';
import type {Run,Work,Transmission} from '../types';
import { providerName,states,shortId } from '../format';
import {sessionId} from '../sessions';
import { type Point,type Viewport,zoomAt,fit,opacity,edgePath,nodeSize,translateGroup,scalePoints,dragDelta,NODE_WIDTH,NODE_HEIGHT } from '../board';
let {runs,works,edges,selected,storageKey,onselect,onopen,halfLife=30,floor=.15,uiScale=1,actions}: {
 runs:Run[];works:Work[];edges:Transmission[];selected:string;storageKey:string;
 onselect:(id:string)=>void;onopen:(id:string)=>void;halfLife?:number;floor?:number;uiScale?:number;actions?:Snippet;
} = $props();
let root:HTMLDivElement;
let points=$state<Record<string,Point>>({});
let view=$state<Viewport>({pan:{x:20,y:40},zoom:1});
let width=$state(800),height=$state(550),now=$state(Date.now()),ready=$state(false);
let loadedKey='';
let pointers=new Map<number,Point>();
let drag:{id:number;node:string|null;group:string|null;last:Point;start:Point;moved:boolean}|null=null;
let pinchDistance=0;
let lastDrag=0;
function persist(){try{localStorage.setItem(storageKey,JSON.stringify({points,view,selected}));}catch{/* Storage may be disabled. */}}
$effect(()=>{
 const key=storageKey;
 if(key!==loadedKey){
  loadedKey=key;points={};view={pan:{x:20,y:40},zoom:1};
  try {const saved=JSON.parse(localStorage.getItem(key)??'null');if(saved){points=saved.points??{};view=saved.view??view;}}catch{/* New layout. */}
 }
 const next={...points};let changed=false,baseY=30;
 for(const work of works){
  const group=runs.filter(r=>r.work_id===work.id);
  const columns=new Map<number,number>();
  for(const run of group){
   let depth=0,parent=run.parent_session_id;const seen=new Set([sessionId(run)]);
   while(parent&&!seen.has(parent)){seen.add(parent);const p=group.find(r=>sessionId(r)===parent);if(!p)break;depth++;parent=p.parent_session_id;}
   const row=columns.get(depth)??0;columns.set(depth,row+1);
   if(!next[sessionId(run)]){next[sessionId(run)]={x:20+depth*360,y:baseY+40+row*145};changed=true;}
  }
  baseY+=Math.max(260,Math.max(0,...columns.values())*145+70);
 }
 if(changed)points=next;
});
onMount(()=>{
 ready=true;
 const observer=new ResizeObserver(entries=>{width=entries[0].contentRect.width;height=entries[0].contentRect.height;});
 observer.observe(root);
 const timer=setInterval(()=>{if(document.visibilityState==='visible')now=Date.now();},500);
 return()=>{observer.disconnect();clearInterval(timer);};
});
const displayPoints=$derived(scalePoints(points,uiScale));
const visible=$derived(runs.filter(r=>{const p=displayPoints[sessionId(r)],size=nodeSize(r.agent_kind,uiScale);return p && (p.x+size.width)*view.zoom+view.pan.x>=-80 && p.x*view.zoom+view.pan.x<=width+80 && (p.y+size.height)*view.zoom+view.pan.y>=-80 && p.y*view.zoom+view.pan.y<=height+80;}));
const visibleEdges=$derived(edges.filter(e=>{
 if(!e.from_run_id||!displayPoints[e.from_run_id]||!displayPoints[e.to_run_id])return false;
 const a=displayPoints[e.from_run_id],b=displayPoints[e.to_run_id];
 return (Math.max(a.x,b.x)+NODE_WIDTH*uiScale)*view.zoom+view.pan.x>=0 && Math.min(a.x,b.x)*view.zoom+view.pan.x<=width &&
 (Math.max(a.y,b.y)+NODE_HEIGHT*uiScale)*view.zoom+view.pan.y>=0 && Math.min(a.y,b.y)*view.zoom+view.pan.y<=height;
}));
const groups=$derived(works.map(w=>{
 const ps=runs.filter(r=>r.work_id===w.id).map(r=>{const p=displayPoints[sessionId(r)];return p?{...p,...nodeSize(r.agent_kind,uiScale)}:null;}).filter(p=>p!==null);
 if(!ps.length)return null;
 const x=Math.min(...ps.map(p=>p.x))-16*uiScale,y=Math.min(...ps.map(p=>p.y))-38*uiScale;
 return {work:w,x,y,w:Math.max(...ps.map(p=>p.x+p.width))+16*uiScale-x,h:Math.max(...ps.map(p=>p.y+p.height))+16*uiScale-y};
}).filter(g=>g!==null));
function local(event:PointerEvent|WheelEvent):Point{const box=root.getBoundingClientRect();return{x:event.clientX-box.left,y:event.clientY-box.top};}
function down(event:PointerEvent,node:string|null=null,group:string|null=null){
 if(event.button!==0)return;
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
  pinchDistance=distance;lastDrag=Date.now();return;
 }
 if(!drag||drag.id!==event.pointerId)return;
 const dx=current.x-previous.x,dy=current.y-previous.y;
 if(Math.hypot(current.x-drag.start.x,current.y-drag.start.y)>4)drag.moved=true;
 if(!drag.moved)return;
 if(drag.node)points=translateGroup(points,[drag.node],dragDelta({x:dx,y:dy},view.zoom,uiScale));
 else if(drag.group)points=translateGroup(points,runs.filter(r=>r.work_id===drag!.group).map(sessionId),dragDelta({x:dx,y:dy},view.zoom,uiScale));
 else view={...view,pan:{x:view.pan.x+dx,y:view.pan.y+dy}};
 drag.last=current;
}
function up(event:PointerEvent){pointers.delete(event.pointerId);if(drag?.moved)lastDrag=Date.now();drag=null;pinchDistance=0;persist();}
function wheel(event:WheelEvent){event.preventDefault();if(event.ctrlKey||event.metaKey)view=zoomAt(view,local(event),view.zoom*Math.exp(-event.deltaY*.008));else view={...view,pan:{x:view.pan.x-event.deltaX,y:view.pan.y-event.deltaY}};persist();}
function scale(factor:number){view=zoomAt(view,{x:width/2,y:height/2},view.zoom*factor);persist();}
function fitAll(){view=fit([...runs.filter(r=>displayPoints[sessionId(r)]).map(r=>({...displayPoints[sessionId(r)],...nodeSize(r.agent_kind,uiScale)})),...groups.map(g=>({x:g.x,y:g.y,width:g.w,height:g.h}))],root.clientWidth,root.clientHeight);persist();}
function dimensions(id:string){return nodeSize(runs.find(r=>sessionId(r)===id)?.agent_kind??'session',uiScale);}
function select(id:string){if(Date.now()-lastDrag<200)return;onselect(id);persist();}
</script>
<div class="board-wrap">
 <div class="board-toolbar">
  <span class="board-count">{works.length}개 업무 · {runs.length}개 세션</span><div class="spacer"></div>
  <div class="board-tools" role="group" aria-label="캔버스 보기">
   <button class="icon-button" aria-label="축소" onclick={()=>scale(.8)}>−</button>
   <span>{Math.round(view.zoom*100)}%</span>
   <button class="icon-button" aria-label="확대" onclick={()=>scale(1.25)}>+</button>
   <button onclick={fitAll}>전체 보기</button>
  </div>
  <div class="board-actions">{@render actions?.()}</div>
 </div>
 <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (The canvas has keyboard zoom and focusable node buttons.) -->
 <div bind:this={root} class="board" role="application" aria-label="세션 캔버스" tabindex="0"
  onpointerdown={(e)=>down(e)} onpointermove={move} onpointerup={up} onpointercancel={up} onwheel={wheel}
  onkeydown={(e)=>{if(e.target!==root)return;if(e.key==='+')scale(1.25);else if(e.key==='-')scale(.8);else if(e.key==='0')fitAll();}}>
  {#if ready}
  <div class="world" style:transform={'translate('+view.pan.x+'px,'+view.pan.y+'px) scale('+view.zoom+')'}>
   {#each groups as group(group.work.id)}
    <div role="button" tabindex="0" aria-label={group.work.title+' 그룹 이동'} onpointerdown={(e)=>down(e,null,group.work.id)} onkeydown={(e)=>{const d=({ArrowLeft:{x:-20,y:0},ArrowRight:{x:20,y:0},ArrowUp:{x:0,y:-20},ArrowDown:{x:0,y:20}} as Record<string,Point>)[e.key];if(d){e.preventDefault();points=translateGroup(points,runs.filter(r=>r.work_id===group.work.id).map(sessionId),d);persist();}}} class="work-group" style:left={group.x+'px'} style:top={group.y+'px'} style:width={group.w+'px'} style:height={group.h+'px'}>
     <span>{shortId(group.work.id)} · {group.work.title}</span>
    </div>
   {/each}
   <svg class="connections" aria-hidden="true">
    <defs><marker id="arrow" markerWidth="9" markerHeight="7" refX="8" refY="3.5" orient="auto"><path d="M 0 0 L 9 3.5 L 0 7 z" fill="currentColor" /></marker></defs>
    {#each runs.filter(r=>r.parent_session_id&&displayPoints[r.parent_session_id]&&displayPoints[sessionId(r)]) as child(child.id)}
     <path d={edgePath(displayPoints[child.parent_session_id!],displayPoints[sessionId(child)],false,dimensions(child.parent_session_id!),dimensions(sessionId(child)))} stroke="currentColor" fill="none" stroke-width="1" stroke-dasharray="4 5" opacity=".25" />
    {/each}
    {#each visibleEdges as edge(edge.id)}
     {@const from=displayPoints[edge.from_run_id!]}{@const to=displayPoints[edge.to_run_id]}
     <g class:reply={edge.kind==='reply'} style:opacity={opacity(edge.sent_at,now,halfLife,floor)}>
      <path d={edgePath(from,to,edge.kind==='reply',dimensions(edge.from_run_id!),dimensions(edge.to_run_id))} stroke="currentColor" fill="none" stroke-width="2" marker-end="url(#arrow)" />
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
     <span class="node-bottom"><span class={'status-text '+run.state}>● {states[run.state]}</span><span>{shortId(sessionId(run))}</span></span>
    </button>
   {/each}
  </div>
  {/if}
  {#if !runs.length}<div class="empty-board">등록된 세션 없음</div>{/if}
 </div>
 <div class="board-legend"><span><i></i>최근 전송</span><span><i class="faded"></i>시간 경과</span><span>┄ 부모 연결</span></div>
</div>
