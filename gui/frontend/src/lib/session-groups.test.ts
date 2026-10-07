import {it,expect} from 'vitest';
import type {Run,Transmission} from './types';
import {arrangeReferences,placeNewReferences,groupIds,inGroup,referenceConnections,sessionReferences,UNGROUPED} from './session-groups';
import {translateGroup} from './board';
const a={id:'turn-a',session_id:'a',work_id:'one',parent_session_id:null} as Run;
const b={id:'turn-b',session_id:'b',work_id:'one',parent_session_id:'a'} as Run;
const memberships=[{session_id:'a',work_ids:['one','two']},{session_id:'b',work_ids:['one','two']}];
it('preserves legacy positions and separates reference positions from conversation identity',()=>{
 const refs=sessionReferences([a,b],memberships),points=arrangeReferences(refs);
 expect(refs.filter(r=>r.session_id==='a')).toHaveLength(2);
 expect(new Set(refs.map(r=>r.id)).size).toBe(2);
 expect(refs.find(r=>r.group_id==='one'&&r.session_id==='a')!.reference_id).toBe('a');
 const other=refs.filter(r=>r.group_id==='two').map(r=>r.reference_id);
 const moved=translateGroup(points,other,{x:20,y:40});
 expect(moved.a).toEqual(points.a);expect(moved.b).toEqual(points.b);
 for(const id of other)expect(moved[id]).toEqual({x:points[id].x+20,y:points[id].y+40});
});
it('draws one connection across shared memberships and one in the focused group',()=>{
 const refs=sessionReferences([a,b],memberships),edges=[{id:'e',from_run_id:'a',to_run_id:'b'} as Transmission];
 const all=referenceConnections(refs,edges);expect(all).toHaveLength(1);expect(all[0].from_reference).toBe('a');
 const focus=referenceConnections(refs.filter(r=>r.group_id==='two'),edges);expect(focus).toHaveLength(1);
 expect(focus[0].from_reference).not.toBe('a');
 expect(referenceConnections(refs,[{...edges[0],to_run_id:'a'}])).toEqual([]);
 expect(referenceConnections(refs.filter(r=>r.session_id==='a'),edges)).toEqual([]);
});
it('an empty explicit membership stays ungrouped while old sessions use the original work',()=>{
 expect(groupIds(a)).toEqual(['one']);expect(groupIds(a,[{session_id:'a',work_ids:[]}])).toEqual([]);
 expect(inGroup(a,UNGROUPED,[{session_id:'a',work_ids:[]}])).toBe(true);
 expect(sessionReferences([a],[{session_id:'a',work_ids:[]}])[0].group_id).toBe(UNGROUPED);
 expect(a.work_id).toBe('one');
});

it('new references avoid saved nodes without resetting previous positions',()=>{
 const other={...b,id:'c',session_id:'c',work_id:'two'};
 const old=arrangeReferences(sessionReferences([a,other]));
 const refs=sessionReferences([a,other],[{session_id:'a',work_ids:['one','two']}]);
 const placed=placeNewReferences(refs,old),alias=refs.find(r=>r.session_id==='a'&&r.group_id==='two')!;
 expect(placed.a).toEqual(old.a);expect(placed.c).toEqual(old.c);
 expect(placed[alias.reference_id].x).toBeGreaterThan(old.c.x+252);
 expect(placeNewReferences(refs,placed)).toBe(placed);
});
