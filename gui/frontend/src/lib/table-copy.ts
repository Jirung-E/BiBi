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
function escapeHtml(text:string):string{return text.replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;');}
function markdownInline(node:Node):string {
 if(node.nodeType===Node.TEXT_NODE)return escapeHtml(node.textContent??'').replaceAll('\\','&#92;').replace(/([`*_[\]~|])/g,'\\$1').replace(/\r?\n/g,' ');
 if(!(node instanceof HTMLElement))return '';
 const children=()=>[...node.childNodes].map(markdownInline).join('');
 switch(node.tagName){
  case 'BR':return '<br>';
  case 'STRONG':case 'B':return '**'+children()+'**';
  case 'EM':case 'I':return '*'+children()+'*';
  case 'DEL':case 'S':return '~~'+children()+'~~';
  case 'CODE':{
   const text=node.textContent??'';
   // GFM splits table cells before parsing code spans. HTML code avoids losing
   // literal backslashes before pipes, whitespace, and multiline code content.
   if(!text||text.includes('|')||text.trim()!==text||/[\r\n]/.test(text))return '<code>'+escapeHtml(text).replace(/[\\`*_[\]~|]/g,c=>'&#'+c.charCodeAt(0)+';').replace(/\r?\n/g,'&#10;')+'</code>';
   const fence='`'.repeat(Math.max(0,...[...text.matchAll(/`+/g)].map(m=>m[0].length))+1),pad=text.startsWith('`')||text.endsWith('`')?' ':'';
   return fence+pad+text+pad+fence;
  }
  case 'A':{
   const href=node.getAttribute('href');if(!href)return children();
   const url=escapeHtml(href.replace(/[\s<>|]/g,c=>encodeURIComponent(c)));
   const title=node.getAttribute('title');
   return '['+children()+'](<'+url+'>'+(title?' "'+escapeHtml(title).replaceAll('\\','\\\\').replaceAll('"','&quot;').replaceAll('|','&#124;').replace(/\r?\n/g,' ')+'"':'')+')';
  }
  case 'P':case 'LI':return children()+'<br>';
  default:return children();
 }
}
export function tableMarkdown(table:HTMLTableElement):string {
 const rows=[...table.rows],columns=Math.max(0,...rows.map(row=>row.cells.length));
 if(!columns)throw new Error('표에 복사할 내용이 없습니다.');
 const line=(values:string[])=>'| '+Array.from({length:columns},(_,i)=>values[i]??'').join(' | ')+' |';
 const cells=(row:HTMLTableRowElement)=>[...row.cells].map(cell=>[...cell.childNodes].map(markdownInline).join('').replace(/^[ \t]+|[ \t]+$/g,space=>[...space].map(c=>'&#'+c.charCodeAt(0)+';').join('')));
 const alignment=Array.from({length:columns},(_,i)=>({left:':---',center:':---:',right:'---:'}[rows[0].cells[i]?.getAttribute('align')??'']??'---'));
 return [line(cells(rows[0])),line(alignment),...rows.slice(1).map(row=>line(cells(row)))].join('\n');
}
type ClipboardValue={text:string;html?:string};
function legacyCopy(value:ClipboardValue):boolean {
 const active=document.activeElement as HTMLElement|null,selection=document.getSelection();
 const ranges=selection?[...Array(selection.rangeCount)].map((_,i)=>selection.getRangeAt(i).cloneRange()):[];
 const control=active instanceof HTMLTextAreaElement||active instanceof HTMLInputElement?active:null;
 const start=control?.selectionStart,end=control?.selectionEnd;
 const input=document.createElement('textarea');input.value=value.text;input.readOnly=true;input.setAttribute('aria-hidden','true');
 Object.assign(input.style,{position:'fixed',left:'0',top:'0',opacity:'0',pointerEvents:'none'});
 const copy=(event:ClipboardEvent)=>{if(event.clipboardData){event.clipboardData.setData('text/plain',value.text);if(value.html!==undefined)event.clipboardData.setData('text/html',value.html);event.preventDefault();}};
 document.addEventListener('copy',copy);
 try{(active?.closest('dialog')??document.body).append(input);input.focus({preventScroll:true});input.select();return document.execCommand('copy');}
 catch{return false;}
 finally{document.removeEventListener('copy',copy);input.remove();active?.focus({preventScroll:true});if(selection){selection.removeAllRanges();for(const range of ranges)selection.addRange(range);}if(control&&start!==null&&end!==null&&start!==undefined&&end!==undefined){try{control.setSelectionRange(start,end);}catch{/* Non-text inputs. */}}}
}
export async function copyTable(table:HTMLTableElement,format:'table'|'markdown'='table'):Promise<void> {
 const value:ClipboardValue=format==='markdown'?{text:tableMarkdown(table)}:tableClipboard(table);
 if(value.html===undefined&&navigator.clipboard?.writeText){try{await navigator.clipboard.writeText(value.text);return;}catch{/* Use the HTTP/user-gesture fallback. */}}
 if(value.html!==undefined&&navigator.clipboard&&typeof ClipboardItem!=='undefined'){
  try{await navigator.clipboard.write([new ClipboardItem({'text/plain':new Blob([value.text],{type:'text/plain'}),'text/html':new Blob([value.html],{type:'text/html'})})]);return;}catch{/* HTTP/WebView/denied clipboard: use the user's copy gesture below. */}
 }
 // The legacy copy event also supplies HTML on non-secure LAN/Tailscale HTTP.
 if(legacyCopy(value))return;
 if(value.html!==undefined&&navigator.clipboard?.writeText){try{await navigator.clipboard.writeText(value.text);return;}catch{/* Show a truthful error. */}}
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
   const block=document.createElement('div'),tools=document.createElement('div'),status=document.createElement('span');
   block.className='table-block';tools.className='table-tools';status.setAttribute('role','status');status.className='sr-only';
   table.replaceWith(block);block.append(tools,table);tools.append(status);
   const buttons:HTMLButtonElement[]=[];
   for(const [format,label] of [['table','표 복사'],['markdown','마크다운 복사']] as const){
    const button=document.createElement('button');button.className='table-copy';button.type='button';button.textContent=label;button.setAttribute('aria-label',label);tools.append(button);buttons.push(button);
    button.onclick=async()=>{
     for(const b of buttons)b.disabled=true;status.className='sr-only';status.setAttribute('role','status');status.textContent='';
     try{await copyTable(table,format);if(!block.isConnected)return;button.textContent='복사됨';status.textContent=format==='markdown'?'표를 마크다운으로 복사했습니다.':'표를 복사했습니다.';const timer=setTimeout(()=>{button.textContent=label;status.textContent='';timers.delete(timer);},2000);timers.add(timer);}
     catch(error){if(!block.isConnected)return;status.className='error';status.setAttribute('role','alert');status.textContent=error instanceof Error?error.message:String(error);}
     finally{for(const b of buttons)b.disabled=false;}
    };
   }
  }
 };
 void sync(html);
 return {update(value:string){void sync(value);},destroy(){disposed=true;generation++;for(const timer of timers)clearTimeout(timer);timers.clear();}};
}
