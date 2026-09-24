import { describe,it,expect } from 'vitest';
import { opacity,zoomAt,fit,NODE_WIDTH,NODE_HEIGHT,nodeSize,scalePoints,translateGroup,dragDelta,resizeViewport } from './board';
import {draftKey,loadDraft,saveDraft} from './drafts';
describe('canvas and delivery contracts',()=>{
 it('ages by the immutable send timestamp',()=>{
   expect(opacity(1000,1000)).toBe(1);expect(opacity(1000,31000)).toBeCloseTo(.575);
   expect(opacity(1000,301000)).toBeCloseTo(.15083);expect(opacity(1000,0)).toBe(1);
 });
 it('zooms around the same pointer world coordinate',()=>{
   const before={pan:{x:25,y:-30},zoom:.8},point={x:300,y:180},after=zoomAt(before,point,1.5);
   expect((point.x-before.pan.x)/before.zoom).toBeCloseTo((point.x-after.pan.x)/after.zoom);
   expect(fit([{x:0,y:0},{x:900,y:500}],390,600).zoom).toBeLessThan(1);
 });
 it('never shares a draft across server or run',()=>{
   const entries=new Map<string,string>();
   const storage={getItem:(k:string)=>entries.get(k)??null,setItem:(k:string,v:string)=>entries.set(k,v)};
   const key=draftKey('server-a','p','w','run-a');saveDraft(key,{text:'이 실행에 보낼 질문',pending:null},storage);
   expect(loadDraft(key,storage).text).not.toBe('');
   expect(loadDraft(draftKey('server-b','p','w','run-a'),storage).text).toBe('');
   expect(loadDraft(draftKey('server-a','p','w','run-b'),storage).text).toBe('');
 });
});

import { submissionId } from './drafts';
it('generates a durable transmission ID without secure-context randomUUID',()=>{
  const first=submissionId(),second=submissionId();
  expect(first).toMatch(/^submission_[0-9a-f]{32}$/);expect(first).not.toBe(second);
});

it('fits a large board without cutting off distant runs and zooms back smoothly',()=>{
  const points=Array.from({length:500},(_,i)=>({x:(i%10)*360,y:Math.floor(i/10)*480}));
  const width=390,height=500,view=fit(points,width,height);
  for(const point of points){
    expect(point.x*view.zoom+view.pan.x).toBeGreaterThanOrEqual(0);
    expect(point.y*view.zoom+view.pan.y).toBeGreaterThanOrEqual(0);
    expect((point.x+NODE_WIDTH)*view.zoom+view.pan.x).toBeLessThanOrEqual(width);
    expect((point.y+NODE_HEIGHT)*view.zoom+view.pan.y).toBeLessThanOrEqual(height);
  }
  expect(zoomAt(view,{x:width/2,y:height/2},view.zoom*1.25).zoom).toBeCloseTo(view.zoom*1.25);
});

it.each([.5,1,1.5,2])('keeps parent/child and sibling nodes apart at UI scale %s',scale=>{
 const base={parent:{x:20,y:70},child:{x:380,y:70},sibling:{x:380,y:215}};
 const shown=scalePoints(base,scale),parent=nodeSize('session',scale),child=nodeSize('subagent',scale);
 expect(shown.child.x-shown.parent.x-parent.width).toBeGreaterThan(0);
 expect(shown.sibling.y-shown.child.y-child.height).toBeGreaterThan(0);
 const moved=translateGroup(base,['parent','child'],dragDelta({x:80,y:40},.8,scale));
 const after=scalePoints(moved,scale);
 expect((after.parent.x-shown.parent.x)*.8).toBeCloseTo(80);
 expect(after.child.x-after.parent.x).toBeCloseTo(shown.child.x-shown.parent.x);
 expect(moved.sibling).toEqual(base.sibling);
 expect(base.parent).toEqual({x:20,y:70});
});


describe('inspector and sidebar resize',()=>{
 const frame={view:{pan:{x:80,y:-120},zoom:.75},size:{width:1200,height:800}};
 const worldCenter=(view:typeof frame.view,size:typeof frame.size)=>({x:(size.width/2-view.pan.x)/view.zoom,y:(size.height/2-view.pan.y)/view.zoom});
 it('keeps the visible world center and all previously visible corners inside the new viewport',()=>{
  for(const size of [{width:880,height:800},{width:1200,height:520},{width:600,height:400}]){
   const view=resizeViewport(frame,size),center=worldCenter(view,size);
   expect(center.x).toBeCloseTo(worldCenter(frame.view,frame.size).x);
   expect(center.y).toBeCloseTo(worldCenter(frame.view,frame.size).y);
   for(const x of [0,frame.size.width])for(const y of [0,frame.size.height]){
    const shown={x:(x-frame.view.pan.x)/frame.view.zoom*view.zoom+view.pan.x,y:(y-frame.view.pan.y)/frame.view.zoom*view.zoom+view.pan.y};
    expect(shown.x).toBeGreaterThanOrEqual(-.001);expect(shown.x).toBeLessThanOrEqual(size.width+.001);
    expect(shown.y).toBeGreaterThanOrEqual(-.001);expect(shown.y).toBeLessThanOrEqual(size.height+.001);
   }
  }
 });
 it('reverses animation frames without accumulating zoom drift',()=>{
  for(let repeat=0;repeat<20;repeat++){
   for(const width of [1150,1030,920,880,900,1060,1200]){
    expect(worldCenter(resizeViewport(frame,{width,height:800}),{width,height:800}).x).toBeCloseTo(worldCenter(frame.view,frame.size).x);
   }
   expect(resizeViewport(frame,frame.size)).toEqual(frame.view);
  }
 });
 it('preserves an intentional pan and zoom made while the inspector is open',()=>{
  const size={width:880,height:800},shrunk=resizeViewport(frame,size);
  const moved=zoomAt({...shrunk,pan:{x:shrunk.pan.x+100,y:shrunk.pan.y-50}},{x:440,y:400},shrunk.zoom*1.25);
  const rebased={view:moved,size};
  const restored=resizeViewport(rebased,frame.size);
  expect(worldCenter(restored,frame.size).x).toBeCloseTo(worldCenter(moved,size).x);
  expect(resizeViewport(rebased,size)).toEqual(moved);
  expect(resizeViewport(frame,{width:0,height:0})).toEqual(frame.view);
 });
});
