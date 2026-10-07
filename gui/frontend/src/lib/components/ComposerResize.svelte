<script lang="ts">
import {untrack} from 'svelte';
let {target}:{target?:HTMLTextAreaElement}=$props();
const storageKey='bibi:composerHeight';
function read(){try{const n=Number(localStorage.getItem(storageKey));return Number.isFinite(n)&&n>=3&&n<=48?n:null;}catch{return null;}}
let preferred=$state<number|null>(read()),active=$state(false),size=$state(0),minimum=$state(0),maximum=$state(0);
let unit=14,drag:{id:number;y:number;height:number;preferred:number|null}|null=null,measure=()=>{};
function save(){try{if(preferred===null)localStorage.removeItem(storageKey);else localStorage.setItem(storageKey,String(preferred));}catch{/* Device preference is optional. */}}
function change(px:number){preferred=Math.max(minimum,Math.min(maximum,px))/unit;measure();}
function reset(){preferred=null;measure();save();}
function down(event:PointerEvent){
 if(event.button!==0||!target)return;event.preventDefault();measure();
 const el=event.currentTarget as HTMLElement;el.focus({preventScroll:true});
 try{el.setPointerCapture(event.pointerId);}catch{/* Synthetic pointers have no capture. */}
 drag={id:event.pointerId,y:event.clientY,height:target.getBoundingClientRect().height,preferred};active=true;
}
function move(event:PointerEvent){if(drag?.id===event.pointerId)change(drag.height+drag.y-event.clientY);}
function finish(event:PointerEvent){
 if(drag?.id!==event.pointerId)return;drag=null;active=false;save();
 const el=event.currentTarget as HTMLElement;if(el.hasPointerCapture(event.pointerId))el.releasePointerCapture(event.pointerId);
}
function key(event:KeyboardEvent){
 if(event.key==='Escape'&&drag){preferred=drag.preferred;drag=null;active=false;measure();event.preventDefault();return;}
 const step=(event.shiftKey?3:1)*unit;
 if(!['ArrowUp','ArrowDown','Home','End','Enter'].includes(event.key))return;
 event.preventDefault();if(event.key==='Enter'){reset();return;}
 change(event.key==='Home'?minimum:event.key==='End'?maximum:size+(event.key==='ArrowUp'?step:-step));save();
}
$effect(()=>{
 const el=target,form=el?.closest('form');if(!el||!form)return;
 let frame=0,disposed=false;
 measure=()=>{
  if(disposed)return;
  unit=parseFloat(getComputedStyle(document.documentElement).fontSize)||14;
  const panel=form.closest<HTMLElement>('.conversation-panel');
  // Preserve the user's preferred height; only the displayed height shrinks with the viewport.
  const budget=panel?panel.clientHeight*(matchMedia('(max-height:600px)').matches?.5:.55):Math.min(innerHeight*.65,form.parentElement?.clientHeight||innerHeight*.65);
  const overhead=Math.max(0,form.scrollHeight-el.offsetHeight);
  minimum=parseFloat(getComputedStyle(el).minHeight)||3.5*unit;
  maximum=Math.max(minimum,Math.min(48*unit,budget-overhead-2));
  if(preferred===null){el.style.removeProperty('height');el.style.removeProperty('max-height');}
  else{const px=Math.max(minimum,Math.min(maximum,preferred*unit))+'px';if(el.style.height!==px)el.style.height=px;if(el.style.maxHeight!==px)el.style.maxHeight=px;}
  size=el.getBoundingClientRect().height;
 };
 const schedule=()=>{cancelAnimationFrame(frame);frame=requestAnimationFrame(measure);};
 const observer=new ResizeObserver(schedule);observer.observe(form);observer.observe(el);if(form.parentElement)observer.observe(form.parentElement);
 window.addEventListener('resize',schedule);window.visualViewport?.addEventListener('resize',schedule);untrack(measure);
 return()=>{disposed=true;cancelAnimationFrame(frame);observer.disconnect();window.removeEventListener('resize',schedule);window.visualViewport?.removeEventListener('resize',schedule);drag=null;active=false;measure=()=>{};};
});
</script>
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Keyboard-operable adjustable separator.) -->
<div class="composer-resize" class:dragging={active} role="separator" tabindex="0" aria-label="입력창 높이" aria-controls={target?.id} aria-orientation="horizontal"
 aria-valuemin={Math.round(minimum)} aria-valuemax={Math.round(maximum)} aria-valuenow={Math.round(size)} aria-valuetext={Math.round(size)+'픽셀'}
 title="위로 끌어 입력창 확대 · 방향키로 조절 · 두 번 클릭하거나 Enter로 초기화"
 onpointerdown={down} onpointermove={move} onpointerup={finish} onpointercancel={finish} onlostpointercapture={finish} onkeydown={key} ondblclick={reset}></div>
<style>
.composer-resize{position:absolute;inset-inline:var(--panel-padding);top:0;display:flex;align-items:center;justify-content:center;height:.65rem;cursor:ns-resize;touch-action:none;user-select:none;border-radius:var(--control-radius);}
.composer-resize::before{content:'';width:2.5rem;height:.2rem;border-radius:var(--control-radius);background:var(--muted);opacity:.55;}
.composer-resize::after{content:'';position:absolute;inset:-.2rem 0;}
.composer-resize:hover::before,.composer-resize.dragging::before,.composer-resize:focus-visible::before{opacity:1;}
.composer-resize:focus-visible{outline:2px solid var(--accent);outline-offset:0;}
</style>
