import {mount,tick,unmount} from 'svelte';
import MermaidBlock,{type DiagramMode} from './components/MermaidBlock.svelte';

// Unchanged Markdown blocks retain their DOM during streaming. Keep their
// diagram component too; only detached/changed sources are unmounted.
export function mermaidBlocks(node:HTMLElement,html:string){
 const modes=new Map<number,DiagramMode>();
 type Entry={instance:ReturnType<typeof mount>;index:number};
 const instances=new Map<HTMLElement,Entry>();let generation=0,disposed=false;
 const clear=()=>{for(const entry of instances.values())void unmount(entry.instance);instances.clear();};
 const sync=async(value:string)=>{
  const current=++generation;await tick();if(disposed||current!==generation)return;
  for(const [host,entry] of instances)if(!node.contains(host)){void unmount(entry.instance);instances.delete(host);}
  if(!/language-mermaid/i.test(value)){clear();return;}
  const blocks=[...node.querySelectorAll<HTMLElement>('.mermaid-host,pre > code')].filter(el=>el.classList.contains('mermaid-host')||(!el.closest('.mermaid-host')&&[...el.classList].some(name=>name.toLowerCase()==='language-mermaid')));
  blocks.forEach((code,index)=>{
   const retained=instances.get(code);if(retained){retained.index=index;return;}
   if(code.classList.contains('mermaid-host'))return;
   const source=code.textContent??'',host=document.createElement('div');host.className='mermaid-host';code.parentElement!.replaceWith(host);
   const entry={} as Entry;entry.index=index;entry.instance=mount(MermaidBlock,{target:host,props:{source,mode:modes.get(index)??'preview',onmode:(mode:DiagramMode)=>modes.set(entry.index,mode)}});instances.set(host,entry);
  });
  for(const index of modes.keys())if(index>=blocks.length)modes.delete(index);
 };
 void sync(html);return {update(value:string){void sync(value);},destroy(){disposed=true;generation++;clear();}};
}
