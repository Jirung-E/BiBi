<script lang="ts">
import {onMount} from 'svelte';
import type {Window} from '@tauri-apps/api/window';
let {onerror}:{onerror:(message:string)=>void}=$props();
let nativeWindow:Window|undefined;
let ready=$state(false),maximized=$state(false);
onMount(()=>{
 let active=true;
 void import('@tauri-apps/api/window').then(({getCurrentWindow})=>{if(active){nativeWindow=getCurrentWindow();ready=true;void sync();}}).catch(error=>{if(active)onerror(String(error));});
 async function sync(){if(!nativeWindow)return;try{const value=await nativeWindow!.isMaximized();if(active)maximized=value;}catch(error){if(active)onerror(String(error));}}
 void sync();window.addEventListener('resize',sync);window.addEventListener('focus',sync);
 return()=>{active=false;window.removeEventListener('resize',sync);window.removeEventListener('focus',sync);};
});
async function control(action:'minimize'|'toggleMaximize'|'close'){
 try{await nativeWindow?.[action]();if(action==='toggleMaximize'&&nativeWindow)maximized=await nativeWindow.isMaximized();}
 catch(error){onerror(String(error));}
}
</script>
<div class="window-controls" aria-label="창 제어">
 <button aria-label="창 최소화" title="최소화" disabled={!ready} onclick={()=>control('minimize')}><svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg></button>
 <button aria-label={maximized?'이전 창 크기로':'창 최대화'} title={maximized?'이전 크기로':'최대화'} disabled={!ready} onclick={()=>control('toggleMaximize')}><svg viewBox="0 0 10 10" aria-hidden="true">{#if maximized}<path d="M2.5 2.5v-2h7v7h-2m-7-5h7v7h-7z" />{:else}<path d="M.5.5h9v9h-9z" />{/if}</svg></button>
 <button class="window-close" aria-label="창 닫기" title="닫기" disabled={!ready} onclick={()=>control('close')}><svg viewBox="0 0 10 10" aria-hidden="true"><path d="m.5.5 9 9m0-9-9 9" /></svg></button>
</div>
