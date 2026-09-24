import {slide,fade} from 'svelte/transition';
import {cubicOut} from 'svelte/easing';
import {refreshScrollbars} from './scrollbars';

function duration(node:Element){
 if(matchMedia('(prefers-reduced-motion: reduce)').matches)return 0;
 const raw=getComputedStyle(node).getPropertyValue('--panel-duration').trim();
 const value=parseFloat(raw);
 return Number.isFinite(value)?value*(raw.endsWith('ms')?1:1000):260;
}
export function reveal(node:Element){return slide(node,{duration:duration(node),easing:cubicOut});}
export function surfaceFade(node:Element){return fade(node,{duration:duration(node),easing:cubicOut});}
export function drawerReveal(node:Element){
 const width=node.getBoundingClientRect().width,margin=parseFloat(getComputedStyle(node).marginLeft)||0;
 return {duration:duration(node),easing:cubicOut,tick:refreshScrollbars,css:(t:number)=>`margin-left:${margin-(1-t)*width}px;opacity:${t}`};
}

// Native summary still owns keyboard activation and semantics. Keep details
// open during the closing frames so its content can actually shrink out.
export function disclosure(node:HTMLDetailsElement){
 let animation:Animation|undefined,expanded=node.open;
 const overflow=node.style.overflow;
 const summary=node.querySelector('summary');
 function toggle(event:MouseEvent){
  if(!summary?.contains(event.target as Node))return;
  event.preventDefault();
  const start=node.getBoundingClientRect().height;
  if(animation){animation.onfinish=null;animation.cancel();animation=undefined;}
  expanded=!expanded;
  const ms=duration(node);
  if(!ms){node.open=expanded;node.style.overflow=overflow;return;}
  node.open=true;
  const style=getComputedStyle(node);
  const end=expanded?node.getBoundingClientRect().height:summary.getBoundingClientRect().height+parseFloat(style.paddingTop)+parseFloat(style.paddingBottom)+parseFloat(style.borderTopWidth)+parseFloat(style.borderBottomWidth);
  node.style.overflow='clip';
  animation=node.animate([{height:start+'px'},{height:end+'px'}],{duration:ms,easing:'cubic-bezier(.22,.68,0,1)'});
  animation.onfinish=()=>{node.open=expanded;node.style.overflow=overflow;animation=undefined;};
 }
 node.addEventListener('click',toggle);
 return {destroy(){node.removeEventListener('click',toggle);animation?.cancel();}};
}
