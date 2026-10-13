<script lang="ts">
import {command} from '../api';
import Icon from './Icon.svelte';
let {id,disabled=false,onchanged=async()=>{}}:{id:string;disabled?:boolean;onchanged?:()=>Promise<void>}=$props();
let busy=$state(false),error=$state(''),notice=$state(''),confirmed=$state(false);
let check=$state<{action:string;requires_confirmation:boolean;entry:{workspace:string;provider_name:string}}|null>(null);
async function open(){busy=true;error='';try{const result=await command<{message:string}>({type:'open_terminal',id,confirmed_stopped:confirmed});notice=result.message;check=null;await onchanged();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
async function prepare(){busy=true;error='';notice='';confirmed=false;try{check=await command({type:'check_terminal',id});if(check&&!check.requires_confirmation)await open();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
</script>
<div class="terminal-action">
{#if !check}<button disabled={busy||disabled} onclick={prepare}><Icon name="terminal" />{busy?'확인 중…':'터미널에서 열기'}</button>
{:else}<span class="terminal-target" title={check.entry.workspace}>{check.entry.provider_name} · {check.entry.workspace}</span>
{#if check.requires_confirmation}<label class="terminal-confirm"><input type="checkbox" bind:checked={confirmed} disabled={busy||disabled} />원래 앱에서 같은 세션을 종료했습니다</label>{/if}
<div class="form-actions"><button disabled={busy||disabled} onclick={()=>check=null}>취소</button><button class="primary" disabled={busy||disabled||check.requires_confirmation&&!confirmed} onclick={open}>{busy?'여는 중…':check.action==='focus'?'원래 창 열기':'터미널에서 열기'}</button></div>{/if}
{#if notice}<p role="status">{notice}</p>{/if}{#if error}<p class="error" role="alert">{error}</p>{/if}
</div>
<style>
.terminal-action{display:flex;flex-direction:column;gap:.5rem;min-width:0}.terminal-action p{margin:0}.terminal-target{font-size:.9rem;color:var(--muted);overflow-wrap:anywhere}.terminal-confirm{display:flex;align-items:center;gap:.5rem;font-size:.9rem}.terminal-confirm input{width:auto;flex:0 0 auto}
</style>
