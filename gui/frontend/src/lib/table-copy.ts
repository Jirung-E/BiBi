import DOMPurify from 'dompurify';
import {tick} from 'svelte';

export function tableClipboard(table:HTMLTableElement):{text:string;html:string} {
 const rows=[...table.rows].map(row=>[...row.cells].map(cell=>{
  const copy=cell.cloneNode(true) as HTMLElement;
  for(const br of copy.querySelectorAll('br'))br.replaceWith('\n');
  for(const block of copy.querySelectorAll('p,li'))block.append('\n');
  const text=(copy.textContent??'').trim().replace(/\r\n?/g,'\n');
  return /[\t\n"]/.test(text)?'"'+text.replaceAll('"','""')+'"':text;
 }).join('\t')).join('\n');
 const html=DOMPurify.sanitize(table.outerHTML,{ALLOWED_TAGS:['table','thead','tbody','tfoot','tr','th','td','br','p','strong','em','code','del'],ALLOWED_ATTR:[],ALLOW_DATA_ATTR:false});
 return {text:rows,html};
}
function legacyCopy(value:{text:string;html:string}):boolean {
 const active=document.activeElement as HTMLElement|null,selection=document.getSelection();
 const ranges=selection?[...Array(selection.rangeCount)].map((_,i)=>selection.getRangeAt(i).cloneRange()):[];
 const control=active instanceof HTMLTextAreaElement||active instanceof HTMLInputElement?active:null;
 const start=control?.selectionStart,end=control?.selectionEnd;
 const input=document.createElement('textarea');input.value=value.text;input.readOnly=true;input.setAttribute('aria-hidden','true');
 Object.assign(input.style,{position:'fixed',left:'0',top:'0',opacity:'0',pointerEvents:'none'});
 const copy=(event:ClipboardEvent)=>{if(event.clipboardData){event.clipboardData.setData('text/plain',value.text);event.clipboardData.setData('text/html',value.html);event.preventDefault();}};
 document.addEventListener('copy',copy);
 try{(active?.closest('dialog')??document.body).append(input);input.focus({preventScroll:true});input.select();return document.execCommand('copy');}
 catch{return false;}
 finally{document.removeEventListener('copy',copy);input.remove();active?.focus({preventScroll:true});if(selection){selection.removeAllRanges();for(const range of ranges)selection.addRange(range);}if(control&&start!==null&&end!==null&&start!==undefined&&end!==undefined){try{control.setSelectionRange(start,end);}catch{/* Non-text inputs. */}}}
}
export async function copyTable(table:HTMLTableElement):Promise<void> {
 const value=tableClipboard(table);
 if(navigator.clipboard&&typeof ClipboardItem!=='undefined'){
  try{await navigator.clipboard.write([new ClipboardItem({'text/plain':new Blob([value.text],{type:'text/plain'}),'text/html':new Blob([value.html],{type:'text/html'})})]);return;}catch{/* HTTP/WebView/denied clipboard: use the user's copy gesture below. */}
 }
 // The legacy copy event also supplies HTML on non-secure LAN/Tailscale HTTP.
 if(legacyCopy(value))return;
 if(navigator.clipboard?.writeText){try{await navigator.clipboard.writeText(value.text);return;}catch{/* Show a truthful error. */}}
 throw new Error('표를 복사하지 못했습니다. 브라우저의 클립보드 권한을 확인하거나 셀을 선택해 복사하세요.');
}
export function tableCopies(node:HTMLElement,html:string){
 let generation=0,disposed=false;
 const timers=new Set<ReturnType<typeof setTimeout>>();
 const sync=async(value:string)=>{
  const id=++generation;await tick();if(disposed||generation!==id)return;
  for(const timer of timers)clearTimeout(timer);timers.clear();
  if(!/<table[ >]/i.test(value))return;
  for(const table of node.querySelectorAll('table')){
   if(table.parentElement?.classList.contains('table-block'))continue;
   const block=document.createElement('div'),tools=document.createElement('div'),button=document.createElement('button'),status=document.createElement('span');
   block.className='table-block';tools.className='table-tools';button.className='table-copy';button.type='button';button.textContent='표 복사';button.setAttribute('aria-label','표 복사');status.setAttribute('role','status');status.className='sr-only';
   table.replaceWith(block);block.append(tools,table);tools.append(status,button);
   button.onclick=async()=>{
    button.disabled=true;status.className='sr-only';status.setAttribute('role','status');status.textContent='';
    try{await copyTable(table);if(!block.isConnected)return;button.textContent='복사됨';status.textContent='표를 복사했습니다.';const timer=setTimeout(()=>{button.textContent='표 복사';status.textContent='';timers.delete(timer);},2000);timers.add(timer);}
    catch(error){if(!block.isConnected)return;status.className='error';status.setAttribute('role','alert');status.textContent=error instanceof Error?error.message:String(error);}
    finally{button.disabled=false;}
   };
  }
 };
 void sync(html);
 return {update(value:string){void sync(value);},destroy(){disposed=true;generation++;for(const timer of timers)clearTimeout(timer);timers.clear();}};
}
