<script lang="ts">
import {onMount,onDestroy,untrack} from 'svelte';
import {command,request} from '../api';
import {age,states} from '../format';
import {scrollbars} from '../scrollbars';
import TerminalAction from './TerminalAction.svelte';
import type {Project,ProviderConfig,Provider,Run,RunState} from '../types';
let {projects,providers,initialQuery='',onopen}:{projects:Project[];providers:ProviderConfig[];initialQuery?:string;onopen:(run:Run)=>void|Promise<void>}=$props();
type Entry={id:string;title:string;provider:Provider;provider_name:string;workspace:string;project_key:string|null;project_name:string;host_id:string;run_id:string|null;state:RunState;updated_at:number;excerpt:string;last_user:string;last_assistant:string;terminal_supported:boolean;terminal_reason:string|null;external:boolean};
let query=$state(untrack(()=>initialQuery)),project=$state(''),provider=$state(''),selected=$state('');
let items=$state<Entry[]>([]),warnings=$state<string[]>([]),more=$state(false),pending=$state(false),opening=$state(false),error=$state('');
let revision=$state(0);
let input=$state<HTMLInputElement>();let generation=0,alive=true;
const entry=$derived(items.find(e=>e.id===selected)??items[0]);
onMount(()=>{const frame=requestAnimationFrame(()=>input?.focus());return()=>cancelAnimationFrame(frame);});
onDestroy(()=>{alive=false;generation++;});
$effect(()=>{void revision;const q=query,p=project,s=provider,g=++generation;pending=true;error='';const timer=setTimeout(async()=>{
 try{const params=new URLSearchParams({q});if(p)params.set('project',p);if(s)params.set('provider',s);const result=await request<{items:Entry[];warnings:string[];has_more:boolean}>('/api/sessions/catalog?'+params);if(alive&&g===generation){items=result.items;warnings=result.warnings;more=result.has_more;if(!items.some(e=>e.id===selected))selected=items[0]?.id??'';}}
 catch(e){if(alive&&g===generation)error=e instanceof Error?e.message:String(e);}
 finally{if(alive&&g===generation)pending=false;}
 },180);return()=>clearTimeout(timer);});
async function open(){if(!entry||opening||pending)return;opening=true;error='';try{const run=await command<Run>({type:'open_catalog_session',id:entry.id});await onopen(run);}catch(e){error=e instanceof Error?e.message:String(e);}finally{opening=false;}}
function keys(e:KeyboardEvent){if(!['ArrowUp','ArrowDown'].includes(e.key)||e.isComposing||pending)return;e.preventDefault();const current=Math.max(0,items.findIndex(v=>v.id===selected));selected=items[Math.max(0,Math.min(items.length-1,current+(e.key==='ArrowDown'?1:-1)))]?.id??'';}
</script>
<div class="session-catalog">
<form onsubmit={e=>{e.preventDefault();void open();}}><input bind:this={input} type="search" aria-label="세션 검색어" placeholder="이름, 대화 내용, 작업 폴더" bind:value={query} maxlength="160" autocomplete="off" onkeydown={keys} /></form>
<div class="catalog-filters"><select aria-label="검색 프로젝트" bind:value={project}><option value="">전체 프로젝트</option>{#each projects as p}<option value={p.id}>{p.name}</option>{/each}</select><select aria-label="검색 제공자" bind:value={provider}><option value="">전체 제공자</option>{#each providers as p}<option value={p.id}>{p.name}</option>{/each}</select></div>
<small class="muted" role="status">{pending?'검색 중…':`${items.length}${more?'+':''}개`}</small>
{#if error}<p class="error" role="alert">{error}</p>{/if}
<div class="catalog-results" use:scrollbars aria-label="전체 세션 검색 결과" aria-busy={pending}>
{#each items as item(item.id)}<button class="catalog-result" class:chosen={entry?.id===item.id} aria-pressed={entry?.id===item.id} disabled={pending||opening} onclick={()=>selected=item.id} ondblclick={()=>{selected=item.id;void open();}}><strong>{item.title}</strong><small>{item.provider_name} · {item.project_name} · {states[item.state]??item.state} · {age(item.updated_at)}</small><span class="catalog-path">{item.host_id} · {item.workspace}</span><span>{item.excerpt}</span></button>
{:else}{#if !pending&&!error}<p class="muted">일치하는 세션이 없습니다.</p>{/if}{/each}
</div>
{#if entry}<section class="catalog-preview" aria-label="선택한 세션 미리보기"><div class="catalog-preview-heading"><strong title={entry.title}>{entry.title}</strong><button disabled={pending||opening} onclick={open}>{opening?'여는 중…':'BiBi에서 보기'}</button></div>
<div class="catalog-messages" use:scrollbars>{#if entry.last_user}<div><small>마지막 질문</small><p>{entry.last_user}</p></div>{/if}{#if entry.last_assistant}<div><small>마지막 답변</small><p>{entry.last_assistant}</p></div>{/if}</div>
{#key entry.id}{#if entry.terminal_supported}<TerminalAction id={entry.id} disabled={pending||opening} onchanged={async()=>{revision++;}} />{:else}<small class="muted">{entry.terminal_reason}</small>{/if}{/key}</section>{/if}
{#if warnings.length}<details class="catalog-warnings"><summary>조회 안내 {warnings.length}개</summary>{#each warnings as warning}<p>{warning}</p>{/each}</details>{/if}
</div>
<style>
.session-catalog{display:flex;flex-direction:column;min-height:0;gap:.6rem}.session-catalog form input{width:100%;min-width:0}.catalog-filters{display:flex;gap:.5rem}.catalog-filters select{min-width:0;flex:1}.catalog-results{display:flex;flex-direction:column;gap:.3rem;max-height:30dvh;overflow:auto;min-height:0}.catalog-result{flex-shrink:0;display:flex;flex-direction:column;align-items:stretch;text-align:left;white-space:normal;height:auto;gap:.2rem;padding:.6rem .75rem;border-radius:var(--radius);background:var(--surface)}.catalog-result.chosen{background:var(--accent-soft);border-color:var(--accent)}.catalog-result strong,.catalog-result span,.catalog-result small{overflow-wrap:anywhere;display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2;line-clamp:2;overflow:hidden}.catalog-result small,.catalog-path{font-size:.8rem;color:var(--muted)}.catalog-preview{border-top:1px solid var(--border);padding-top:.6rem;display:flex;flex-direction:column;gap:.5rem;min-width:0}.catalog-preview-heading{display:flex;align-items:center;gap:.6rem}.catalog-preview-heading strong{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.catalog-preview-heading button{flex-shrink:0}.catalog-messages{max-height:20dvh;overflow:auto;display:flex;flex-direction:column;gap:.6rem}.catalog-messages p{white-space:pre-wrap;overflow-wrap:anywhere;margin:.2rem 0;font-size:.9rem}.catalog-messages small{color:var(--muted)}.catalog-warnings{font-size:.85rem;overflow-wrap:anywhere}
</style>
