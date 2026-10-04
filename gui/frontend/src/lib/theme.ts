import {get,readonly,writable} from 'svelte/store';
import {isDesktop} from './api';

export type ThemeChoice='system'|'light'|'dark';
export type EffectiveTheme='light'|'dark';
const key='bibi:theme';
const normalize=(value:unknown):ThemeChoice=>value==='light'||value==='dark'?value:'system';
function readChoice():ThemeChoice {
 try{return normalize(localStorage.getItem(key));}catch{return 'system';}
}
function resolve(choice:ThemeChoice):EffectiveTheme {
 return choice==='system'?(typeof matchMedia!=='undefined'&&matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):choice;
}
const choice=writable<ThemeChoice>(readChoice());
const effective=writable<EffectiveTheme>(resolve(get(choice)));
export const theme=readonly(choice),effectiveTheme=readonly(effective);
let nativeUpdate:Promise<unknown>=Promise.resolve();

function apply(value:ThemeChoice){
 const resolved=resolve(value),root=document.documentElement;
 root.dataset.theme=resolved;
 root.style.colorScheme=resolved;
 // Notify SVG/image renderers only after the CSS palette has changed.
 effective.set(resolved);
}
function applyWindow(value:ThemeChoice){
 if(!isDesktop())return;
 // Serialize rapid selections so an older native request cannot win last.
 nativeUpdate=nativeUpdate.then(async()=>{
  const {getCurrentWindow}=await import('@tauri-apps/api/window');
  await getCurrentWindow().setTheme(value==='system'?null:value);
 }).catch(error=>console.warn('Window theme:',error));
}
export function setTheme(value:ThemeChoice){
 const next=normalize(value);
 try{localStorage.setItem(key,next);}catch{/* The current window still changes when storage is unavailable. */}
 choice.set(next);
}
export function initTheme(){
 choice.set(readChoice());
 const unsubscribe=choice.subscribe(value=>{apply(value);applyWindow(value);});
 const media=matchMedia('(prefers-color-scheme: dark)');
 const changed=()=>{if(get(choice)==='system')apply('system');};
 const stored=(event:StorageEvent)=>{
  if(event.key===key||event.key===null)choice.set(normalize(event.newValue));
 };
 media.addEventListener('change',changed);
 window.addEventListener('storage',stored);
 return()=>{unsubscribe();media.removeEventListener('change',changed);window.removeEventListener('storage',stored);};
}
