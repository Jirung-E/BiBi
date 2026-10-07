<script lang="ts">
import {untrack} from 'svelte';
import {command} from '../api';
import {modalDialog} from '../modal';
import {scrollbars} from '../scrollbars';
import {surfaceFade} from '../motion';
import {groupIds} from '../session-groups';
import type {Run,Work,SessionGroups} from '../types';
let {run,works,memberships,onchanged}:{run:Run;works:Work[];memberships:SessionGroups[];onchanged:()=>Promise<void>}=$props();
let opened=$state(false),selected=$state<string[]>([]),expected:string[]=[],busy=$state(false),error=$state('');
let editing=$state<Run>(untrack(()=>run));
function open(){editing=run;expected=[...groupIds(run,memberships)];selected=[...expected];error='';opened=true;}
async function save(){
 if(busy)return;busy=true;error='';
 try{await command({type:'set_session_groups',run_id:editing.id,work_ids:selected,expected_work_ids:expected});await onchanged();opened=false;}
 catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}
}
</script>
<button onclick={open}>그룹 연결</button>
{#if opened}
 <dialog class="modal card group-dialog" use:modalDialog use:scrollbars transition:surfaceFade aria-label="그룹 연결" oncancel={(e)=>{e.preventDefault();if(!busy)opened=false;}}>
  <form onsubmit={(e)=>{e.preventDefault();void save();}}>
   <h2>그룹 연결</h2><p class="group-session-name">{editing.title}</p>
   <fieldset disabled={busy} class="group-options" use:scrollbars><legend class="sr-only">연결할 그룹</legend>
    {#each works.filter(w=>w.project_key===editing.project_key) as work(work.id)}
     <label class="check"><input type="checkbox" value={work.id} bind:group={selected} /><span>{work.title}{#if work.id===editing.work_id}<small>원래 업무</small>{/if}</span></label>
    {/each}
   </fieldset>
   <small class="muted">{selected.length?selected.length+'개 그룹 · 같은 대화를 공유합니다.':'미분류에 표시됩니다.'}</small>
   {#if error}<p class="error" role="alert">{error}</p>{/if}
   <div class="form-actions"><button type="button" disabled={busy} onclick={()=>opened=false}>취소</button><button class="primary" disabled={busy}>{busy?'저장 중…':'저장'}</button></div>
  </form>
 </dialog>
{/if}
<style>
 .group-dialog form{display:flex;flex-direction:column;gap:.75rem;min-width:0}
 .group-dialog h2,.group-session-name{margin:0;overflow-wrap:anywhere}
 .group-options{border:0;padding:0;margin:0;display:flex;flex-direction:column;gap:.5rem;max-height:45dvh;overflow:auto;min-width:0}
 .group-options label{flex-shrink:0;color:var(--text);align-items:flex-start;white-space:normal;padding:.5rem;border-radius:var(--radius);background:var(--surface)}
 .group-options label span{min-width:0;overflow-wrap:anywhere}
 .group-options input{flex-shrink:0;margin-top:.15rem}
 .group-options small{display:block;color:var(--muted)}
</style>
