import {mount,tick,unmount} from 'svelte';
import MermaidBlock,{type DiagramMode} from './components/MermaidBlock.svelte';

// Enhance sanitized code blocks in place, including blocks nested in lists or
// quotes. The source remains text; model output never supplies component HTML.
export function mermaidBlocks(node:HTMLElement,html:string){
 const modes=new Map<number,DiagramMode>();
 let instances:ReturnType<typeof mount>[]=[],generation=0,disposed=false;
 const clear=()=>{for(const instance of instances)void unmount(instance);instances=[];};
 const sync=async(value:string)=>{
  const current=++generation;
  await tick();
  if(disposed||current!==generation)return;
  clear();
  if(!/language-mermaid/i.test(value))return;
  const blocks=[...node.querySelectorAll<HTMLElement>('pre > code')].filter(code=>[...code.classList].some(name=>name.toLowerCase()==='language-mermaid'));
  blocks.forEach((code,index)=>{
   const source=code.textContent??'',host=document.createElement('div');
   host.className='mermaid-host';code.parentElement!.replaceWith(host);
   instances.push(mount(MermaidBlock,{target:host,props:{source,mode:modes.get(index)??'preview',onmode:(mode:DiagramMode)=>modes.set(index,mode)}}));
  });
  for(const index of modes.keys())if(index>=blocks.length)modes.delete(index);
 };
 void sync(html);
 return {update(value:string){void sync(value);},destroy(){disposed=true;generation++;clear();}};
}
