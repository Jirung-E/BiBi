<script lang="ts">
import {onMount,onDestroy} from 'svelte';
import {submissionId} from '../drafts';
import {command} from '../api';
import type {Run} from '../types';
let {run,oncreated}:{run:Run;oncreated:(run:Run)=>Promise<void>}=$props();
type Point={id:string;revision:string;excerpt:string;created_at:number};
let points=$state<Point[]>([]),selected=$state(''),title=$state(''),loading=$state(true),busy=$state(false),error=$state(''),workspace=$state('');
let alive=true,requestId=submissionId();
const point=$derived(points.find(p=>p.id===selected));
onMount(()=>{title=(run.title.slice(0,110)+' · 포크');void load();});
onDestroy(()=>{alive=false;});
async function load(){
 loading=true;error='';requestId=submissionId();
 try{const value=await command<{points:Point[];workspace:string}>({type:'list_fork_points',run_id:run.id});if(alive){points=value.points;workspace=value.workspace;selected=points.at(-1)?.id??'';}}
 catch(e){if(alive)error=e instanceof Error?e.message:String(e);}
 finally{if(alive)loading=false;}
}
async function fork(){
 if(!point||busy)return;
 busy=true;error='';
 try{
  const value=await command<Run>({type:'fork_session',request:{request_id:requestId,run_id:run.id,point_id:point.id,revision:point.revision,title:title.trim()}});
  if(alive)await oncreated(value);
 }catch(e){if(alive)error=e instanceof Error?e.message:String(e);}
 finally{if(alive)busy=false;}
}
</script>
<form class="session-fork-form" aria-busy={loading||busy} onsubmit={e=>{e.preventDefault();void fork();}}>
 {#if loading}<p role="status">분기 지점 불러오는 중…</p>{:else if points.length}
 <label>이름<input bind:value={title} maxlength="120" required disabled={busy} oninput={()=>requestId=submissionId()} /></label>
 <label>분기 지점<select bind:value={selected} disabled={busy} onchange={()=>requestId=submissionId()}>{#each points as p,i}<option value={p.id}>{i+1}. {p.excerpt.slice(0,60)}</option>{/each}</select></label>
 {#if point}<blockquote class="fork-excerpt">{point.excerpt}</blockquote>{/if}
 <label>작업 폴더<input readonly value={workspace} /></label>
 <small>이 지점까지의 대화를 복제합니다. 작업 폴더와 파일은 원본과 공유합니다.</small>
 <div class="form-actions"><button type="submit" class="primary" disabled={busy||!point||!title.trim()}>{busy?'포크 중…':'여기서 포크'}</button></div>
 {:else if !error}<p class="muted">포크할 수 있는 완료된 답변이 없습니다.</p>{/if}
 {#if error}<p class="error" role="alert">{error}</p><button type="button" onclick={load} disabled={busy}>분기 지점 다시 확인</button>{/if}
</form>
<style>
 .session-fork-form{display:flex;flex-direction:column;gap:var(--section-gap);min-width:0}
 .session-fork-form>label,.session-fork-form>.form-actions{margin:0}
 .fork-excerpt{margin:0;padding:var(--section-gap);border-inline-start:.125rem solid var(--accent);white-space:pre-wrap;overflow-wrap:anywhere}
 .session-fork-form small{color:var(--muted)}
</style>
