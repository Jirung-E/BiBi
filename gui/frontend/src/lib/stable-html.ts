// Only accepts the sanitized output of markdown(). Keeping unchanged blocks
// also keeps selection, table controls, and already rendered diagrams alive.
export function htmlPieces(html:string):string[]{
 if(typeof document==='undefined')return [html];
 const t=document.createElement('template');t.innerHTML=html;
 return Array.from(t.content.childNodes).map(n=>{const holder=document.createElement('div');holder.appendChild(n.cloneNode(true));return holder.innerHTML;});
}
function patch(parent:Node,next:Node){
 const nodes=Array.from(next.childNodes);
 for(let i=0;i<nodes.length;i++){
  const fresh=nodes[i],old=parent.childNodes[i];
  if(!old){parent.appendChild(fresh.cloneNode(true));continue;}
  if(old.nodeType!==fresh.nodeType||old.nodeName!==fresh.nodeName){parent.replaceChild(fresh.cloneNode(true),old);continue;}
  if(old.nodeType===Node.TEXT_NODE){if(old.nodeValue!==fresh.nodeValue)old.nodeValue=fresh.nodeValue;continue;}
  if(old instanceof Element&&fresh instanceof Element){
   for(const a of Array.from(old.attributes))if(!fresh.hasAttribute(a.name)&&a.name!=='data-overlay-scrollbars'&&!(a.name==='id'&&a.value.startsWith('bibi-scroll-')))old.removeAttribute(a.name);
   for(const a of Array.from(fresh.attributes))if(old.getAttribute(a.name)!==a.value)old.setAttribute(a.name,a.value);
   patch(old,fresh);
  }
 }
 while(parent.childNodes.length>nodes.length)parent.lastChild?.remove();
}
export function stableHtml(node:HTMLElement,initial:string){
 let previous:string|undefined;
 const update=(html:string)=>{
  if(previous===html)return;previous=html;
  // These enhanced blocks own DOM beyond the sanitized source. Rebuild only
  // the changed block; their parent actions reconcile their own lifecycle.
  if(node.querySelector('.table-block,.mermaid-host,.mermaid-block')){node.innerHTML=html;return;}
  const t=document.createElement('template');t.innerHTML=html;patch(node,t.content);
 };
 update(initial);return {update};
}
