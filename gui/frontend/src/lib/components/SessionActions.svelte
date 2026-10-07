<script lang="ts">
import Icon from './Icon.svelte';
import {reveal} from '../motion';
import type {Snippet} from 'svelte';
import type {Run,Work,SessionGroups} from '../types';
import GroupSelector from './SessionGroups.svelte';
import {command} from '../api';
import {isActive} from '../format';
let {run,onchanged,children,works=[],memberships=[],groupEditing=false,onfork}:{run:Run;onchanged:()=>Promise<void>;children?:Snippet;works?:Work[];memberships?:SessionGroups[];groupEditing?:boolean;onfork?:()=>void} = $props();
let edit=$state(false),removing=$state(false),title=$state(''),busy=$state(false),error=$state('');
async function save(body:unknown){busy=true;error='';try{await command(body);edit=false;removing=false;await onchanged();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
</script>
<div class="session-actions">
 <div class="session-action-buttons">
 {#if onfork}<button class="icon-button" aria-label="세션 포크" title={run.host_id!=='local'?'세션이 저장된 호스트에서 포크하세요':run.agent_kind==='subagent'?'부모 대화에서 포크하세요':run.provider==='command'?'이 제공자는 포크를 지원하지 않습니다':'세션 포크'} disabled={run.host_id!=='local'||run.agent_kind==='subagent'||run.provider==='command'} onclick={onfork}><Icon name="fork" /></button>{/if}
 {#if groupEditing}<GroupSelector {run} {works} {memberships} {onchanged} />{/if}
 <button aria-label="세션 이름 변경" onclick={()=>{title=run.title;edit=!edit;removing=false;}}>이름 변경</button>
 <button class="danger-button" disabled={busy||(run.origin==='managed'&&(isActive(run.state)||run.state==='queued'))} onclick={()=>{removing=!removing;edit=false;}}>제거</button>
 {@render children?.()}
 </div>
 {#if edit}<form transition:reveal class="inline-confirm" onsubmit={(e)=>{e.preventDefault();void save({type:'rename_session',run_id:run.id,title});}}><input aria-label="새 세션 이름" bind:value={title} required maxlength="120" /><div class="form-actions"><button type="button" onclick={()=>edit=false}>취소</button><button class="primary" disabled={busy}>저장</button></div></form>{/if}
 {#if removing}<div transition:reveal class="inline-confirm"><span>목록에서 제거합니다. 설정에서 복원할 수 있습니다.</span><div class="form-actions"><button onclick={()=>removing=false}>취소</button><button class="danger-button" disabled={busy} onclick={()=>save({type:'set_session_hidden',run_id:run.id,hidden:true})}>제거 확인</button></div></div>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>
