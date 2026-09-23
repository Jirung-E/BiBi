import {it,expect} from 'vitest';
import {readNavigation,navigationUrl} from './navigation';
import {modelSuggestions,recentModels} from './models';
import {nodeSize,translateGroup,fit,edgePath} from './board';
import type {ProviderConfig} from './types';
it('restores view, session and modal from a navigation URL while preserving unrelated parameters',()=>{
 const url=new URL('http://localhost/?x=1');
 const state={view:'conversation' as const,project:'p',run:'a',modal:'settings' as const};
 const next=navigationUrl(url,state);expect(readNavigation(next)).toEqual(state);expect(next.searchParams.get('x')).toBe('1');
 expect(readNavigation(new URL('http://localhost/?view=invalid&modal=anything')).modal).toBe('');
});
it('separates model history by provider and sorts frequent and recent choices independently',()=>{
 const provider={id:'a',models:['manual','popular']} as ProviderConfig;
 const history=[{provider_id:'a',model:'popular',uses:5,last_used:1},{provider_id:'a',model:'recent',uses:1,last_used:5},{provider_id:'b',model:'private',uses:20,last_used:8}];
 expect(modelSuggestions(provider,history)).toEqual(['popular','recent','manual']);expect(recentModels('a',history)[0].model).toBe('recent');expect(modelSuggestions(undefined,history)).toEqual([]);
});
it('moves only the selected group and fits scaled child dimensions',()=>{
 const points={a:{x:10,y:20},b:{x:200,y:20},c:{x:10,y:500}};
 const moved=translateGroup(points,['a','b'],{x:40,y:-20});expect(moved.a).toEqual({x:50,y:0});expect(moved.c).toEqual(points.c);expect(points.a.x).toBe(10);
 const child=nodeSize('subagent',2),parent=nodeSize('session',2);expect(child.width).toBeLessThan(parent.width);
 const view=fit([{...moved.a,...child},{...moved.b,...parent}],390,600);expect((moved.b.x+parent.width)*view.zoom+view.pan.x).toBeLessThan(390);
 expect(edgePath({x:0,y:0},{x:500,y:0},false,child,parent)).toContain('M 392 ');
});
