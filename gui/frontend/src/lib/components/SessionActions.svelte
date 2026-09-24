<script lang="ts">
import {reveal} from '../motion';
import type {Run} from '../types';
import {command} from '../api';
import {isActive} from '../format';
let {run,onchanged}:{run:Run;onchanged:()=>Promise<void>}=$props();
let edit=$state(false),removing=$state(false),title=$state(''),busy=$state(false),error=$state('');
async function save(body:unknown){busy=true;error='';try{await command(body);edit=false;removing=false;await onchanged();}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
</script>
<div class="session-actions">
 <button aria-label="세션 이름 변경" onclick={()=>{title=run.title;edit=!edit;removing=false;}}>이름 변경</button>
 <button class="danger-button" disabled={busy||(run.origin==='managed'&&(isActive(run.state)||run.state==='queued'))} onclick={()=>{removing=!removing;edit=false;}}>제거</button>
 {#if edit}<form transition:reveal class="inline-confirm" onsubmit={(e)=>{e.preventDefault();void save({type:'rename_session',run_id:run.id,title});}}><input aria-label="새 세션 이름" bind:value={title} required maxlength="120" /><button class="primary" disabled={busy}>저장</button><button type="button" onclick={()=>edit=false}>취소</button></form>{/if}
 {#if removing}<div transition:reveal class="inline-confirm"><span>목록에서 제거합니다. 설정에서 복원할 수 있습니다.</span><button class="danger-button" disabled={busy} onclick={()=>save({type:'set_session_hidden',run_id:run.id,hidden:true})}>제거 확인</button><button onclick={()=>removing=false}>취소</button></div>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>
