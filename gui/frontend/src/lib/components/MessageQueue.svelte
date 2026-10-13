<script lang="ts">
import type {QueuedMessage} from '../types';
import {command} from '../api';
import Icon from './Icon.svelte';
import {scrollbars} from '../scrollbars';
let {items,onchanged}:{items:QueuedMessage[];onchanged:()=>Promise<void>}=$props();
let busy=$state(''),error=$state('');
async function change(id:string,cancel:boolean){busy=id;error='';try{await command({type:'change_queued_message',id,cancel});await onchanged();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy='';}}
</script>
{#if items.length}<details class="message-queue" open><summary>대기 메시지 {items.length}</summary><div class="queue-items" use:scrollbars>{#each items as item(item.id)}<div class="queue-item"><span>{item.request.question||'첨부 파일'}{#if item.error}<small class="error">{item.error}</small>{/if}</span>{#if item.state==='blocked'}<button disabled={!!busy} onclick={()=>change(item.id,false)}>재개</button>{/if}<button class="icon-button" aria-label="대기 메시지 취소" title="대기 메시지 취소" disabled={!!busy||item.state==='dispatching'} onclick={()=>change(item.id,true)}><Icon name="close" /></button></div>{/each}</div>{#if error}<p class="error" role="alert">{error}</p>{/if}</details>{/if}
<style>
.message-queue{flex:0 0 auto;max-height:10rem;overflow:auto;padding:.35rem .8rem;border-top:1px solid var(--border);font-size:.85rem}.queue-items{max-height:7rem;overflow:auto}.queue-item{display:flex;align-items:center;gap:.5rem;padding:.3rem 0}.queue-item>span{flex:1;min-width:0;white-space:pre-wrap;overflow-wrap:anywhere}.queue-item small{display:block}.queue-item .icon-button{flex-shrink:0}
</style>
