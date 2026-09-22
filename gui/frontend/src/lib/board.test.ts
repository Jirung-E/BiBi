import { describe,it,expect } from 'vitest';
import { opacity,zoomAt,fit,NODE_WIDTH,NODE_HEIGHT } from './board';
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
