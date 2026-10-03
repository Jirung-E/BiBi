// @vitest-environment jsdom
import {afterEach,expect,it,vi} from 'vitest';
import {conversationScroll,captureReadingPosition} from './conversation-scroll';
afterEach(()=>{vi.unstubAllGlobals();document.body.replaceChildren();});
function fixture(){
 vi.stubGlobal('ResizeObserver',class {observe(){}unobserve(){}disconnect(){}});
 vi.stubGlobal('requestAnimationFrame',()=>1);vi.stubGlobal('cancelAnimationFrame',()=>{});
 const node=document.createElement('div');node.className='messages';const block=document.createElement('article');block.className='message';const diagram=document.createElement('section');diagram.className='mermaid-block';block.append(diagram);node.append(block);document.body.append(node);
 let height=1800,offset=500,following=false;node.scrollTop=400;
 Object.defineProperties(node,{scrollHeight:{get:()=>height},clientHeight:{get:()=>600},clientWidth:{get:()=>500}});
 node.getBoundingClientRect=()=>({top:0,left:0,width:500,height:600} as DOMRect);
 diagram.getBoundingClientRect=()=>({top:offset-node.scrollTop,left:0,width:400,height:200} as DOMRect);
 document.elementFromPoint=()=>diagram;
 const action=conversationScroll(node,{following:()=>following,set:value=>following=value});
 return {node,diagram,action,grow(amount:number){height+=amount;offset+=amount;},follow(value:boolean){following=value;},following:()=>following};
}
it('keeps the reading position when a deferred diagram above it changes height',async()=>{
 const f=fixture(),restore=captureReadingPosition(f.diagram);f.grow(240);restore();expect(f.node.scrollTop).toBe(640);expect(f.following()).toBe(false);f.action.destroy();
});
it('a delayed render cannot undo a newer user scroll or revive tail following',()=>{
 const f=fixture(),restore=captureReadingPosition(f.diagram);f.node.dispatchEvent(new WheelEvent('wheel',{deltaY:-20}));f.node.scrollTop=200;f.grow(240);restore();expect(f.node.scrollTop).toBe(200);expect(f.following()).toBe(false);f.action.destroy();
});
it('tail following and detached messages do not receive a second scroll correction',()=>{
 const f=fixture();f.follow(true);const restore=captureReadingPosition(f.diagram);f.grow(200);restore();expect(f.node.scrollTop).toBe(400);f.follow(false);const detached=captureReadingPosition(f.diagram);f.action.destroy();f.grow(200);detached();expect(f.node.scrollTop).toBe(400);
});
