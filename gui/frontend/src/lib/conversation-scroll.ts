const readingRestores=new WeakMap<HTMLElement,()=>()=>void>();
export function captureReadingPosition(element:HTMLElement){return readingRestores.get(element.closest<HTMLElement>('.messages')!)?.()??(()=>{});}

// Keep the latest answer pinned through deferred message layout and panel motion.
// Explicit scrolling into older history releases the pin; layout changes do not.
export function conversationScroll(node:HTMLElement,options:{following:()=>boolean;set:(value:boolean)=>void}){
 let disposed=false,frame=0,lastTop=0,lastHeight=0,lastWidth=0,lastView=0,inputRevision=0;
 const remember=()=>{lastTop=node.scrollTop;lastHeight=node.scrollHeight;lastWidth=node.clientWidth;lastView=node.clientHeight;};
 const schedule=()=>{if(!frame&&!disposed)frame=requestAnimationFrame(()=>{frame=0;if(options.following())node.scrollTop=node.scrollHeight;remember();});};
 const scroll=()=>{
  const upward=node.scrollTop<lastTop-1&&node.scrollHeight===lastHeight&&node.clientWidth===lastWidth&&node.clientHeight===lastView;
  // Being near the tail is not a request to follow it. A small upward wheel,
  // touch or thumb movement must stay released until the reader returns.
  if(upward)options.set(false);
  else if(node.scrollHeight===lastHeight&&node.clientHeight===lastView&&node.scrollTop>lastTop&&node.scrollHeight-node.scrollTop-node.clientHeight<=2)options.set(true);
  remember();if(options.following())schedule();
 };
 const intent=(event?:Event)=>{
  inputRevision++;
  // End/drag-to-bottom must survive a deferred message becoming taller as it
  // enters the viewport. Other explicit input releases the current pin.
  const end=event instanceof CustomEvent&&event.detail?.axis==='vertical'&&event.detail?.end===true;
  options.set(end);if(end)schedule();
 };
 const wheel=(event:WheelEvent)=>{if(event.deltaY<0)intent();};
 const key=(event:KeyboardEvent)=>{if(['ArrowUp','PageUp','Home'].includes(event.key))intent();};
 const resized=new ResizeObserver(schedule),observed=new Set<Element>();
 const observe=()=>{
  const next=new Set<Element>([node]);
  // Observing every transcript child forces deferred offscreen layout in WebKit.
  const tail=node.querySelector('.message:last-of-type');if(tail)next.add(tail);
  if(node.lastElementChild)next.add(node.lastElementChild);
  for(const item of observed)if(!next.has(item)){resized.unobserve(item);observed.delete(item);}
  for(const item of next)if(!observed.has(item)){resized.observe(item);observed.add(item);}
  schedule();
 };
 // Anchor a visible block while a lazy diagram above it gains its real height.
 // Read one hit-tested element, never all deferred transcript descendants.
 readingRestores.set(node,()=>{
  if(options.following())return ()=>{};
  const box=node.getBoundingClientRect(),revision=inputRevision;
  const element=node.ownerDocument.elementFromPoint(box.left+box.width*.5,box.top+box.height*.5)?.closest<HTMLElement>('.mermaid-block,.message');
  if(!element||!node.contains(element))return ()=>{};
  const top=element.getBoundingClientRect().top;
  return ()=>{
   if(disposed||options.following()||revision!==inputRevision||!node.contains(element))return;
   // Browsers with native scroll anchoring already have a zero delta here.
   node.scrollTop+=element.getBoundingClientRect().top-top;remember();
  };
 });
 const changed=new MutationObserver(observe);changed.observe(node,{childList:true});observe();
 node.addEventListener('scroll',scroll,{passive:true});node.addEventListener('wheel',wheel,{passive:true});
 node.addEventListener('touchstart',intent,{passive:true});node.addEventListener('keydown',key);
 node.addEventListener('scrollintent',intent);
 node.addEventListener('contentvisibilityautostatechange',schedule,true);
 return {destroy(){readingRestores.delete(node);disposed=true;cancelAnimationFrame(frame);resized.disconnect();changed.disconnect();
  node.removeEventListener('scroll',scroll);node.removeEventListener('wheel',wheel);node.removeEventListener('touchstart',intent);node.removeEventListener('keydown',key);node.removeEventListener('scrollintent',intent);node.removeEventListener('contentvisibilityautostatechange',schedule,true);
 }};
}

const layoutLocks=new WeakMap<HTMLElement,()=>void>();
// During a width animation, distant messages keep their measured boxes. This
// prevents WebKit from repeatedly rewrapping the entire transcript. Release at
// the end (or before the user scrolls), so normal selection/search stay intact.
export function freezeOffscreenMessages(node:HTMLElement|undefined){
 if(!node)return;
 layoutLocks.get(node)?.();
 const viewport=node.getBoundingClientRect();
 const boxes=Array.from(node.querySelectorAll<HTMLElement>(':scope > .message')).map(element=>({element,rect:element.getBoundingClientRect()}));
 const frozen=boxes.filter(({rect})=>rect.bottom<viewport.top-viewport.height||rect.top>viewport.bottom+viewport.height);
 for(const {element,rect} of frozen){element.style.contentVisibility='hidden';element.style.containIntrinsicBlockSize=rect.height+'px';}
 let timer:ReturnType<typeof setTimeout>;
 const release=()=>{
  clearTimeout(timer);
  for(const {element} of frozen){element.style.removeProperty('content-visibility');element.style.removeProperty('contain-intrinsic-block-size');}
  for(const type of ['wheel','touchstart','keydown','scrollintent'])node.removeEventListener(type,release);
  layoutLocks.delete(node);
 };
 for(const type of ['wheel','touchstart','keydown','scrollintent'])node.addEventListener(type,release,{once:true,passive:true});
 timer=setTimeout(release,340);layoutLocks.set(node,release);
}
