import type {Run,Transmission} from './types';
export const sessionId=(run:Run|undefined)=>run?.session_id||run?.id||'';
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
