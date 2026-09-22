import {it,expect} from 'vitest';
import type {Run,Transmission} from './types';
import {sessionNodes,sessionEdges} from './sessions';
const run=(id:string,session_id:string,continued_from:string|null=null,parent_session_id:string|null=null)=>({id,session_id,continued_from,parent_session_id,title:id,created_at:1,updated_at:1} as Run);
it('keeps one node across turns and a separate node per expert and native subagent',()=>{
 const a=run('a','main'),b=run('b','main','a'),c=run('c','expert',null,'main'),d=run('d','native',null,'expert');
 const nodes=sessionNodes([b,a,d,c]);expect(nodes).toHaveLength(3);expect(nodes.find(n=>n.session_id==='main')).toMatchObject({id:'b',title:'a'});expect(nodes.find(n=>n.id==='d')?.parent_session_id).toBe('expert');
});
it('maps message arrows to stable sessions and removes same-session arrows',()=>{
 const runs=[run('a','main'),run('b','main','a'),run('e','expert')];
 const edges=[{id:'follow',from_run_id:'a',to_run_id:'b',sent_at:12},{id:'consult',from_run_id:'b',to_run_id:'e',sent_at:25}] as Transmission[];
 expect(sessionEdges(runs,edges)).toEqual([{id:'consult',from_run_id:'main',to_run_id:'expert',sent_at:25}]);
});
