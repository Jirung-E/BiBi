// Track only BiBi entries. history.length also counts pages outside the app and
// cannot tell whether Forward is available. Page state holds the current entry;
// sessionStorage remembers its range across a reload without storing any URLs.
export type HistoryEntry={id:string;index:number};
export type NavigationHistory=HistoryEntry&{last:number};
const prefix='bibi:navigation:';
function entry(value:unknown):HistoryEntry|null {
 if(!value||typeof value!=='object')return null;
 const {id,index}=value as HistoryEntry;
 return typeof id==='string'&&/^[a-z0-9-]{1,80}$/.test(id)&&Number.isSafeInteger(index)&&index>=0?{id,index}:null;
}
export function restoreHistory(value:unknown,previous?:NavigationHistory):NavigationHistory {
 const current=entry(value)??{id:Date.now().toString(36)+'-'+Math.random().toString(36).slice(2),index:0};
 let last=current.index;
 if(previous?.id===current.id)last=Math.max(last,previous.last);
 else try{
  const saved=JSON.parse(sessionStorage.getItem(prefix+current.id)??'null');
  if(Number.isSafeInteger(saved)&&saved>=last)last=saved;
 }catch{/* History still works in this page when storage is unavailable. */}
 return {...current,last};
}
export function historyEntry(history:NavigationHistory):HistoryEntry{return {id:history.id,index:history.index};}
export function pushHistory(history:NavigationHistory):NavigationHistory {
 // Native pushState discards the forward branch; the button must do so too.
 const index=history.index+1,next={...history,index,last:index};
 try{sessionStorage.setItem(prefix+next.id,JSON.stringify(next.last));}catch{/* Optional reload persistence. */}
 return next;
}

type HistoryMarker={entry:HistoryEntry;modal:boolean};
let initial:HistoryMarker|undefined;
export function captureInitialHistory(){
 // SvelteKit deliberately starts with empty page.state on a full page load.
 // Read our native entry marker in the client init hook, before that happens.
 const value=window.history.state?.bibiNavigation,current=entry(value?.entry);
 if(current)initial={entry:current,modal:value.modal===true};
}
export function initialHistory(){const value=initial;initial=undefined;return value;}
export function rememberHistory(history:NavigationHistory,modal:boolean){
 // This changes metadata only: preserve every router field and the current URL.
 // All actual route changes still go through SvelteKit pushState/replaceState.
 window.history.replaceState({...window.history.state,bibiNavigation:{entry:historyEntry(history),modal}},'');
}
