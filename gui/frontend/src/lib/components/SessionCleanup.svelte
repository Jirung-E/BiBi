<script lang="ts">
import {onDestroy,untrack} from 'svelte';
import {command} from '../api';
import {disconnectedSessions,sessionId} from '../sessions';
import {providerName} from '../format';
import {scrollbars} from '../scrollbars';
import type {Snapshot,Project} from '../types';
let {snapshot,project,onchanged,onclose,onrestore}:{snapshot:Snapshot;project:Project;onchanged:()=>Promise<void>;onclose:()=>void;onrestore:()=>void}=$props();
type Result={hidden_session_ids:string[];skipped_run_ids:string[]};
const candidates=$derived(disconnectedSessions(snapshot.runs.filter(r=>r.project_key===project.id),snapshot.approvals));
let selected=$state<string[]>(untrack(()=>candidates.slice(0,2000).map(r=>r.id)));
let busy=$state(false),error=$state(''),syncError=$state(''),result=$state<Result|null>(null),alive=true;
onDestroy(()=>alive=false);
const chosen=$derived(candidates.filter(r=>selected.includes(r.id)));
const allSelected=$derived(candidates.length>0&&chosen.length===Math.min(candidates.length,2000));
async function refresh(){
 syncError='';
 try{await onchanged();}catch(e){if(alive)syncError='목록 갱신 실패: '+(e instanceof Error?e.message:String(e));}
}
async function clean(){
 if(busy||!chosen.length||!snapshot.session_cleanup_v1)return;
 busy=true;error='';syncError='';result=null;
 try{
  const value=await command<Result>({type:'cleanup_disconnected_sessions',project_key:project.id,run_ids:chosen.map(r=>r.id)});
  if(!alive)return;
  result=value;selected=[];await refresh();
 }catch(e){if(alive)error=e instanceof Error?e.message:String(e);}
 finally{if(alive)busy=false;}
}
async function retryRefresh(){if(busy)return;busy=true;await refresh();if(alive)busy=false;}
</script>
<form class="session-cleanup" onsubmit={(e)=>{e.preventDefault();void clean();}}>
 <p class="cleanup-scope"><strong>{project.name}</strong><span>목록에서 숨깁니다. 설정 → 제거한 세션에서 복원할 수 있습니다.</span></p>
 {#if !snapshot.session_cleanup_v1}<p class="error" role="alert">연결된 BiBi 서버를 업데이트해야 일괄 정리할 수 있습니다.</p>{/if}
 {#if candidates.length}
  <label class="check"><input type="checkbox" checked={allSelected} indeterminate={chosen.length>0&&!allSelected} disabled={busy} onchange={(e)=>selected=e.currentTarget.checked?candidates.slice(0,2000).map(r=>r.id):[]} />전체 선택 · {chosen.length}/{candidates.length}</label>
  <div class="cleanup-list" use:scrollbars aria-label="정리할 세션">
   {#each candidates as run(sessionId(run))}<label class="cleanup-item"><input type="checkbox" aria-label={run.title} value={run.id} bind:group={selected} disabled={busy||(!selected.includes(run.id)&&chosen.length>=2000)} /><span><strong>{run.title}</strong><small>{providerName(run,snapshot.providers)} · {snapshot.hosts.find(h=>h.id===run.host_id)?.name??run.host_id}{run.agent_kind==='subagent'?' · 서브에이전트':''}</small></span></label>{/each}
  </div>
 {:else if !result}<p class="muted" role="status">정리할 연결 끊긴 세션이 없습니다.</p>{/if}
 {#if result}<p role="status">{result.hidden_session_ids.length}개 정리됨{result.skipped_run_ids.length?' · '+result.skipped_run_ids.length+'개 제외':''}</p>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
 {#if syncError}<p class="error" role="alert">{syncError}</p><button type="button" disabled={busy} onclick={retryRefresh}>목록 새로고침</button>{/if}
 <div class="form-actions"><button type="button" disabled={busy} onclick={onrestore}>제거한 세션 보기</button><button type="button" onclick={onclose}>{result?'닫기':'취소'}</button>{#if candidates.length}<button class="primary" disabled={busy||!chosen.length||!snapshot.session_cleanup_v1||!!syncError}>{busy?'정리 중…':chosen.length+'개 정리'}</button>{/if}</div>
</form>
