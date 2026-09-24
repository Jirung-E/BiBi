<script lang="ts">
import {onDestroy} from 'svelte';
let {label,value,min,max,unit,axis='x',direction=1,onresize,onactive,oncommit,onreset}:{
 label:string;value:number;min:number;max:number;unit:number;axis?:'x'|'y';direction?:1|-1;
 onresize:(value:number)=>void;onactive:(active:boolean)=>void;oncommit:()=>void;onreset:()=>void;
}=$props();
let drag:{id:number;start:number;value:number}|null=null;
let active=$state(false);
function change(next:number){onresize(Math.max(min,Math.min(max,next)));}
function finish(){if(!drag)return;drag=null;active=false;onactive(false);oncommit();}
function down(event:PointerEvent){
 if(event.button!==0)return;
 event.preventDefault();
 const target=event.currentTarget as HTMLElement;
 target.focus({preventScroll:true});target.setPointerCapture(event.pointerId);
 drag={id:event.pointerId,start:axis==='x'?event.clientX:event.clientY,value};active=true;onactive(true);
}
function move(event:PointerEvent){
 if(drag?.id!==event.pointerId)return;
 change(drag.value+direction*((axis==='x'?event.clientX:event.clientY)-drag.start)/unit);
}
function key(event:KeyboardEvent){
 if(event.key==='Escape'&&drag){event.preventDefault();change(drag.value);finish();return;}
 const step=event.shiftKey?2:.5;
 const delta=axis==='x'?({ArrowLeft:-1,ArrowRight:1} as Record<string,number>)[event.key]:({ArrowUp:-1,ArrowDown:1} as Record<string,number>)[event.key];
 if(delta===undefined&&!['Home','End'].includes(event.key))return;
 event.preventDefault();change(event.key==='Home'?min:event.key==='End'?max:value+delta*direction*step);oncommit();
}
onDestroy(()=>{if(active)onactive(false);});
</script>
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (A focusable adjustable separator follows the ARIA window splitter pattern.) -->
<div class="panel-resize" class:horizontal={axis==='y'} class:dragging={active} role="separator" tabindex="0"
 aria-label={label} aria-orientation={axis==='x'?'vertical':'horizontal'}
 aria-valuemin={Math.round(min*unit)} aria-valuemax={Math.round(max*unit)} aria-valuenow={Math.round(value*unit)} aria-valuetext={Math.round(value*unit)+'픽셀'}
 title="드래그 또는 방향키로 크기 조절 · 두 번 클릭하면 기본 크기"
 onpointerdown={down} onpointermove={move} onpointerup={finish} onpointercancel={finish} onlostpointercapture={finish} onkeydown={key} ondblclick={onreset}></div>
