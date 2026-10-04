<script lang="ts">
import {onMount,tick} from 'svelte';
import {effectiveTheme} from '../theme';
import {captureReadingPosition} from '../conversation-scroll';
import {watchVisible} from '../scrollbars';
import {cachedDiagram,diagramPalette,diagramProblem,renderDiagram,type Diagram} from '../mermaid';
export type DiagramMode='preview'|'code';
let {source,mode='preview',onmode}:{source:string;mode?:DiagramMode;onmode:(mode:DiagramMode)=>void}=$props();
let host:HTMLElement;
let visible=$state(false),result=$state<Diagram>(),error=$state(''),loading=$state(false),previewHeight=$state(10);
const problem=$derived(diagramProblem(source));
function show(value:Diagram|undefined){
 const restore=captureReadingPosition(host);result=value;
 if(value)previewHeight=Math.min(32,value.height/14+2);
 void tick().then(restore);
}
function choose(value:DiagramMode){mode=value;onmode(value);}
onMount(()=>{
 return watchVisible(host,value=>visible=value);
});
$effect(()=>{
 void $effectiveTheme;
 if(!visible||mode!=='preview'||problem){result=undefined;loading=false;return;}
 const palette=diagramPalette(host),saved=cachedDiagram(source,palette);
 show(saved);error='';
 if(saved){loading=false;return;}
 const controller=new AbortController();
 loading=true;
 // Coalesce streaming updates and do not render blocks that leave the viewport.
 const timer=setTimeout(()=>{
  renderDiagram(source,palette,controller.signal).then(value=>{
   if(!controller.signal.aborted){show(value);loading=false;}
  }).catch(()=>{
   if(!controller.signal.aborted){error='미리보기를 만들지 못했습니다. 작성 중이거나 문법이 올바르지 않을 수 있습니다.';loading=false;}
  });
 },180);
 return ()=>{clearTimeout(timer);controller.abort();};
});
</script>

<section class="mermaid-block" bind:this={host} aria-label="Mermaid 다이어그램">
 <div class="mermaid-toolbar">
  <span class="mermaid-label">Mermaid</span>
  <div class="mermaid-modes" role="group" aria-label="Mermaid 보기">
   <button type="button" aria-pressed={mode==='preview'} onclick={()=>choose('preview')}>미리보기</button>
   <button type="button" aria-pressed={mode==='code'} onclick={()=>choose('code')}>코드</button>
  </div>
 </div>
 {#if mode==='code'}
  <pre class="mermaid-source" aria-label="Mermaid 코드"><code>{source}</code></pre>
 {:else if problem||error}
  <div class="mermaid-notice" role="status"><span>{problem||error}</span><button type="button" onclick={()=>choose('code')}>코드 보기</button></div>
 {:else if result}
  <div class="mermaid-preview" aria-label="다이어그램 미리보기" role="region">
   <img src={result.url} alt="Mermaid 다이어그램" width={result.width} height={result.height} style:width={result.width/14+'rem'} draggable="false" />
  </div>
 {:else}
  <div class="mermaid-placeholder" style:height={previewHeight+'rem'} role="status" aria-busy={loading}>{loading?'다이어그램을 그리는 중…':'다이어그램'}</div>
 {/if}
</section>
