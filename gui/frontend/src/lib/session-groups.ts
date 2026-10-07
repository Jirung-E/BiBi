import type {Run,SessionGroups,Transmission} from './types';
import {sessionId} from './sessions';
import {arrangeNodes,nodeSize,type Point} from './board';

export const UNGROUPED='ungrouped';
export type SessionReference=Run & {reference_id:string;group_id:string;shared_count:number};
export function groupIds(run:Run,memberships:SessionGroups[]=[]):string[]{
 return memberships.find(m=>m.session_id===sessionId(run))?.work_ids??[run.work_id];
}
export function inGroup(run:Run,group:string,memberships:SessionGroups[]=[]):boolean{
 const ids=groupIds(run,memberships);return group===UNGROUPED?ids.length===0:ids.includes(group);
}
export function sessionReferences(runs:Run[],memberships:SessionGroups[]=[]):SessionReference[]{
 const saved=new Map(memberships.map(m=>[m.session_id,m.work_ids]));
 return runs.flatMap(run=>{
  const id=sessionId(run),ids=saved.get(id)??[run.work_id];
  return (ids.length?ids:[UNGROUPED]).map(group=>({...run,group_id:group,shared_count:ids.length,
   // Preserve existing device layouts for the original membership.
   reference_id:group===run.work_id?id:'reference:'+JSON.stringify([group,id])}));
 });
}
export function arrangeReferences(refs:SessionReference[]){
 const ids=new Map(refs.map(r=>[JSON.stringify([r.group_id,sessionId(r)]),r.reference_id]));
 return arrangeNodes(refs.map(r=>({id:r.reference_id,work_id:r.group_id,
  parent_session_id:r.parent_session_id?ids.get(JSON.stringify([r.group_id,r.parent_session_id])):undefined})));
}
// Add beside existing group members without moving their saved positions or
// placing a new alias directly on top of an existing node.
export function placeNewReferences(refs:SessionReference[],previous:Record<string,Point>):Record<string,Point>{
 const missing=refs.filter(r=>!previous[r.reference_id]);
 if(!missing.length)return previous;
 const arranged=arrangeReferences(refs);
 if(!Object.keys(previous).length)return arranged;
 const next={...previous};
 for(const ref of missing){
  const group=refs.filter(r=>r.group_id===ref.group_id&&next[r.reference_id]);
  const point=group.length?{x:Math.max(...group.map(r=>next[r.reference_id].x+nodeSize(r.agent_kind).width))+108,y:Math.min(...group.map(r=>next[r.reference_id].y))}:{...arranged[ref.reference_id]};
  const size=nodeSize(ref.agent_kind);
  const overlaps=()=>refs.some(r=>{const p=next[r.reference_id],s=nodeSize(r.agent_kind);return p&&point.x<p.x+s.width+24&&point.x+size.width+24>p.x&&point.y<p.y+s.height+24&&point.y+size.height+24>p.y;});
  while(overlaps())point.x+=360;
  next[ref.reference_id]=point;
 }
 return next;
}

// One visible reference pair per actual connection; never form a Cartesian
// product of memberships. The same rule applies to a focused group's subset.
export function referenceConnections(refs:SessionReference[],edges:Transmission[]){
 const bySession=new Map<string,SessionReference[]>();
 for(const ref of refs){const id=sessionId(ref);bySession.set(id,[...(bySession.get(id)??[]),ref]);}
 const preferred=(items:SessionReference[])=>items.find(r=>r.group_id===r.work_id)??[...items].sort((a,b)=>a.group_id.localeCompare(b.group_id))[0];
 return edges.flatMap(edge=>{
  if(!edge.from_run_id||edge.from_run_id===edge.to_run_id)return [];
  const from=bySession.get(edge.from_run_id),to=bySession.get(edge.to_run_id);
  if(!from?.length||!to?.length)return [];
  const common=from.filter(a=>to.some(b=>a.group_id===b.group_id));
  const a=common.length?preferred(common):preferred(from);
  const b=to.find(b=>b.group_id===a.group_id)??preferred(to);
  return [{...edge,from_reference:a.reference_id,to_reference:b.reference_id}];
 });
}
