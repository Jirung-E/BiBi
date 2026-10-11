// Only activate a file already present in this rendered answer. No path fetch.
export function attachmentLinks(node:HTMLElement){
 const click=(event:MouseEvent)=>{
  const link=(event.target as Element).closest<HTMLAnchorElement>('a[href^="#bibi-attachment-"]');
  if(!link||!node.contains(link))return;
  event.preventDefault();
  const id=link.getAttribute('href')!.slice('#bibi-attachment-'.length);
  if(!/^att_[a-f0-9]{32}$/.test(id))return;
  node.closest('.message')?.querySelector<HTMLButtonElement>(`button[data-attachment-id="${id}"]`)?.click();
 };
 node.addEventListener('click',click);return {destroy:()=>node.removeEventListener('click',click)};
}
