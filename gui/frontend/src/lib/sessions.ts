import type {Run,Transmission,Approval} from './types';
export const sessionId=(run:Run|undefined)=>run?.session_id||run?.id||'';
export function sameConversation(a:Run|null|undefined,b:Run|null|undefined):boolean {
 return !!a&&!!b&&sessionId(a)===sessionId(b)&&a.project_key===b.project_key&&a.host_id===b.host_id&&a.provider===b.provider&&a.provider_id===b.provider_id;
}
export function sessionNodes(runs:Run[]):Run[] {
 const groups=new Map<string,Run[]>();
 for(const run of runs){const id=sessionId(run);groups.set(id,[...(groups.get(id)??[]),run]);}
 return [...groups.values()].map(group=>{
  const parents=new Set(group.map(r=>r.continued_from).filter(Boolean));
  const latest=group.filter(r=>!parents.has(r.id)).sort((a,b)=>b.created_at-a.created_at||b.updated_at-a.updated_at)[0];
  const first=group.find(r=>!r.continued_from)??group[0];
  return {...latest,title:first.title};
 });
}
export function sessionEdges(runs:Run[],edges:Transmission[]):Transmission[] {
 const ids=new Map(runs.map(r=>[r.id,sessionId(r)]));
 return edges.map(e=>({...e,from_run_id:e.from_run_id?ids.get(e.from_run_id)??null:null,to_run_id:ids.get(e.to_run_id)??''}))
  .filter(e=>e.from_run_id&&e.to_run_id&&e.from_run_id!==e.to_run_id);
}

export function disconnectedSessions(runs:Run[],approvals:Approval[]):Run[] {
 const protectedRuns=new Set(approvals.filter(a=>['pending','sending','uncertain'].includes(a.state)).map(a=>a.run_id));
 const blocked=new Set(runs.filter(r=>['queued','running','waiting_user','waiting_expert','uncertain'].includes(r.state)||protectedRuns.has(r.id)).map(sessionId));
 return sessionNodes(runs).filter(r=>r.state==='disconnected'&&!blocked.has(sessionId(r)));
}
