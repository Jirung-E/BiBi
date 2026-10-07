<script lang="ts">
import {onDestroy,onMount,untrack} from 'svelte';
import {request} from '../api';
import {scrollbars} from '../scrollbars';
import {providerName,stateLabel} from '../format';
import type {Project,ProviderConfig,Run} from '../types';
let {externalResume=false,project,providers,onopen,initialQuery=''}:{externalResume?:boolean;initialQuery?:string;project:Project;providers:ProviderConfig[];onopen:(run:Run)=>void}=$props();
let query=$state(untrack(()=>initialQuery)),hits=$state<{run:Run;excerpt:string}[]>([]),pending=$state(false),error=$state(''),generation=0;
let input=$state<HTMLInputElement>();
onMount(()=>{const frame=requestAnimationFrame(()=>input?.focus({preventScroll:true}));return()=>cancelAnimationFrame(frame);});
let alive=true;
onDestroy(()=>{alive=false;generation++;});
$effect(()=>{
 const q=query,p=project.id,id=++generation;pending=true;error='';
 const timer=setTimeout(async()=>{
  try{const values=await request<typeof hits>('/api/sessions/search?'+new URLSearchParams({project:p,q}));if(alive&&id===generation)hits=values;}
  catch(e){if(alive&&id===generation){hits=[];error=e instanceof Error?e.message:String(e);}}
  finally{if(alive&&id===generation)pending=false;}
 },180);
 return()=>clearTimeout(timer);
});
</script>
<div class="session-search">
 <form onsubmit={e=>{e.preventDefault();if(!pending&&hits[0])onopen(hits[0].run);}}><input bind:this={input} type="search" aria-label="세션 검색어" placeholder="이름, 모델, 대화 내용" bind:value={query} maxlength="160" autocomplete="off" /></form>
 <small class="muted" role="status" title={project.name}>{pending?'검색 중…':error?'검색 실패':`${hits.length}${hits.length===100?'+':''}개`}</small>
 {#if error}<p class="error" role="alert">{error}</p>{/if}
 <div class="search-results" use:scrollbars aria-label="검색 결과" aria-busy={pending}>
  {#each hits as hit(hit.run.id)}<button class="search-result" disabled={pending} onclick={()=>onopen(hit.run)}><strong title={hit.run.title}>{hit.run.title}</strong><small>{providerName(hit.run,providers)} · {hit.run.model} · {stateLabel(hit.run)} · {hit.run.capabilities.continue_session?.supported?'이어가기':externalResume&&hit.run.origin==='external'&&hit.run.host_id==='local'&&['claude','codex'].includes(hit.run.provider)&&hit.run.agent_kind!=='subagent'&&!hit.run.parent_session_id?'종료 후 이어받기':'기록 보기'}</small><span>{hit.excerpt}</span></button>
  {:else}{#if !pending&&!error}<p class="muted">일치하는 세션이 없습니다.</p>{/if}{/each}
 </div>
</div>
<style>
 .session-search{display:flex;flex-direction:column;gap:.75rem;min-height:0}
 .search-results{max-height:55dvh;overflow:auto;display:flex;flex-direction:column;gap:.5rem}
 .search-result{display:flex;flex-direction:column;align-items:stretch;text-align:left;white-space:normal;height:auto;flex-shrink:0;gap:.35rem;border-radius:var(--radius);padding:.75rem}
 .search-result strong,.search-result span,.search-result small{overflow-wrap:anywhere}
 .search-result strong,.search-result span{display:-webkit-box;-webkit-box-orient:vertical;line-clamp:2;-webkit-line-clamp:2;overflow:hidden}
 .search-result small{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
 .search-result span{font-size:.9rem;line-height:1.45}
</style>
