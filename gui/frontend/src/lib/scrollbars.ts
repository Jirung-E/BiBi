// OpenGuild-style overlay thumbs: native scrolling, no reserved layout width.
// Each owner keeps its own scroll position. Dialog thumbs live in the same
// top layer as their owner; all geometry is clipped to visible ancestor boxes.
let nextId=0;
const refreshers=new Set<()=>void>();
export function refreshScrollbars(){for(const refresh of refreshers)refresh();}

type Axis='vertical'|'horizontal';
export function scrollbars(node:HTMLElement){
 const doc=node.ownerDocument;
 const originalId=node.id;
 if(!node.id)node.id='bibi-scroll-'+(++nextId);
 node.classList.add('overlay-scrollable');
 const layer=doc.createElement('div');layer.className='scrollbar-layer';
 const dialog=node.closest('dialog');
 (dialog??doc.body).appendChild(layer);
 let contentDirty=true;
 let frame=0,hideTimer:ReturnType<typeof setTimeout>|undefined,disposed=false;
 let dragging:Axis|null=null;
 const bars=(['vertical','horizontal'] as const).map(axis=>{
  const thumb=doc.createElement('div');thumb.className='overlay-thumb '+axis;
  thumb.setAttribute('role','scrollbar');thumb.tabIndex=0;
  thumb.setAttribute('aria-controls',node.id);thumb.setAttribute('aria-orientation',axis);
  thumb.setAttribute('aria-label',(node.getAttribute('aria-label')??'내용')+(axis==='vertical'?' 세로 스크롤':' 가로 스크롤'));
  thumb.setAttribute('aria-valuemin','0');layer.appendChild(thumb);
  return {axis,thumb,max:0,travel:0,start:0,position:0};
 });
 function reveal(){
  layer.classList.add('visible');clearTimeout(hideTimer);
  hideTimer=setTimeout(()=>{if(!dragging)layer.classList.remove('visible');},1200);
 }
 function schedule(){if(!frame&&!disposed)frame=requestAnimationFrame(()=>{frame=0;if(contentDirty){observe();contentDirty=false;}measure();});}
 function measure(){
  const modal=Array.from(doc.querySelectorAll('dialog[open]')).at(-1);
  const rect=node.getBoundingClientRect(),origin=layer.getBoundingClientRect();
  const style=getComputedStyle(node);
  let left=rect.left+node.clientLeft,top=rect.top+node.clientTop;
  let right=left+node.clientWidth,bottom=top+node.clientHeight;
  for(let parent=node.parentElement;parent&&parent!==doc.body;parent=parent.parentElement){
   const css=getComputedStyle(parent),box=parent.getBoundingClientRect();
   if(css.overflowX!=='visible'){left=Math.max(left,box.left+parent.clientLeft);right=Math.min(right,box.left+parent.clientLeft+parent.clientWidth);}
   if(css.overflowY!=='visible'){top=Math.max(top,box.top+parent.clientTop);bottom=Math.min(bottom,box.top+parent.clientTop+parent.clientHeight);}
  }
  left=Math.max(0,left);top=Math.max(0,top);right=Math.min(innerWidth,right);bottom=Math.min(innerHeight,bottom);
  const rem=parseFloat(getComputedStyle(doc.documentElement).fontSize),inset=.2*rem;
  for(const bar of bars){
   const vertical=bar.axis==='vertical',view=vertical?node.clientHeight:node.clientWidth;
   const content=vertical?node.scrollHeight:node.scrollWidth,offset=vertical?node.scrollTop:node.scrollLeft;
   const overflow=vertical?style.overflowY:style.overflowX;
   bar.max=Math.max(0,content-view);
   const visible=node.getClientRects().length>0&&!node.closest('[inert]')&&(!modal||modal.contains(node));
   const needed=visible&&/auto|scroll/.test(overflow)&&bar.max>1&&right-left>1&&bottom-top>1;
   bar.thumb.hidden=!needed;if(!needed)continue;
   const span=(vertical?bottom-top:right-left)-2*inset;
   if(span<=0){bar.thumb.hidden=true;continue;}
   bar.thumb.setAttribute('aria-controls',node.id);
   const length=Math.min(span,Math.max(2*rem,span*view/content));bar.travel=Math.max(0,span-length);
   const position=bar.travel*Math.max(0,Math.min(1,offset/bar.max));
   bar.thumb.style.cssText=vertical
    ?'left:'+(right-origin.left-inset)+'px;top:'+(top-origin.top+inset+position)+'px;height:'+length+'px'
    :'left:'+(left-origin.left+inset+position)+'px;top:'+(bottom-origin.top-inset)+'px;width:'+length+'px';
   bar.thumb.setAttribute('aria-valuemax',String(Math.round(bar.max)));
   bar.thumb.setAttribute('aria-valuenow',String(Math.round(offset)));
  }
 }
 for(const bar of bars){
  const vertical=bar.axis==='vertical';
  const set=(value:number)=>{if(vertical)node.scrollTop=value;else node.scrollLeft=value;schedule();reveal();};
  bar.thumb.addEventListener('pointerdown',e=>{
   if(e.button!==0)return;e.preventDefault();e.stopPropagation();measure();
   dragging=bar.axis;bar.start=vertical?e.clientY:e.clientX;bar.position=vertical?node.scrollTop:node.scrollLeft;
   bar.thumb.classList.add('dragging');bar.thumb.setPointerCapture(e.pointerId);reveal();
  });
  bar.thumb.addEventListener('pointermove',e=>{if(dragging===bar.axis&&bar.travel>0)set(bar.position+((vertical?e.clientY:e.clientX)-bar.start)*bar.max/bar.travel);});
  const end=()=>{dragging=null;bar.thumb.classList.remove('dragging');reveal();};
  bar.thumb.addEventListener('pointerup',end);bar.thumb.addEventListener('pointercancel',end);bar.thumb.addEventListener('lostpointercapture',end);
  bar.thumb.addEventListener('keydown',e=>{
   const current=vertical?node.scrollTop:node.scrollLeft,page=(vertical?node.clientHeight:node.clientWidth)*.9;
   const step=parseFloat(getComputedStyle(node).fontSize)*3;
   const deltas:Record<string,number>={ArrowDown:step,ArrowRight:step,ArrowUp:-step,ArrowLeft:-step,PageDown:page,PageUp:-page};
   const value=e.key==='Home'?0:e.key==='End'?bar.max:e.key in deltas?current+deltas[e.key]:null;
   if(value!==null){e.preventDefault();e.stopPropagation();set(value);}
  });
 }
 const onScroll=()=>{schedule();reveal();};
 const ro=new ResizeObserver(schedule);
 // Also observe content and ancestors: stream growth and animated panel moves
 // can change scroll height or position without resizing the owner itself.
 const observed=new Set<Element>();
 const observe=()=>{
  const next=new Set<Element>();for(let e:HTMLElement|null=node;e;e=e.parentElement)next.add(e);for(const child of node.children)if(child!==layer)next.add(child);
  for(const e of observed)if(!next.has(e)){ro.unobserve(e);observed.delete(e);}
  for(const e of next)if(!observed.has(e)){ro.observe(e);observed.add(e);}
 };
 const mo=new MutationObserver(records=>{if(records.some(r=>!(r.target instanceof Element?r.target:r.target.parentElement)?.closest('.scrollbar-layer'))){contentDirty=true;schedule();}});
 mo.observe(node,{childList:true,subtree:true,characterData:true});
 node.addEventListener('scroll',onScroll,{passive:true});node.addEventListener('input',schedule);
 node.addEventListener('pointerenter',reveal);node.addEventListener('focusin',reveal);
 doc.addEventListener('scroll',schedule,{capture:true,passive:true});window.addEventListener('resize',schedule);
 refreshers.add(schedule);schedule();
 return {destroy(){
  disposed=true;cancelAnimationFrame(frame);clearTimeout(hideTimer);ro.disconnect();mo.disconnect();
  node.removeEventListener('scroll',onScroll);node.removeEventListener('input',schedule);node.removeEventListener('pointerenter',reveal);node.removeEventListener('focusin',reveal);
  doc.removeEventListener('scroll',schedule,true);window.removeEventListener('resize',schedule);refreshers.delete(schedule);
  layer.remove();node.classList.remove('overlay-scrollable');if(!originalId)node.removeAttribute('id');
 }};
}

// Markdown is sanitized HTML, so its code blocks/tables cannot use Svelte actions.
export function contentScrollbars(node:HTMLElement){
 const entries=new Map<HTMLElement,ReturnType<typeof scrollbars>>();
 const sync=()=>{
  for(const [element,action] of entries)if(!node.contains(element)){action.destroy();entries.delete(element);}
  for(const element of node.querySelectorAll<HTMLElement>('pre,table'))if(!entries.has(element))entries.set(element,scrollbars(element));
 };
 const observer=new MutationObserver(sync);observer.observe(node,{subtree:true,childList:true});sync();
 return {destroy(){observer.disconnect();for(const action of entries.values())action.destroy();}};
}
