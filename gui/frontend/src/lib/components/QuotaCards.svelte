<script lang="ts">
import type {Quota,ProviderConfig} from '../types';
import {providers,age,dateTime} from '../format';
let {quotas,now=Date.now(),expanded=false,compact=false,connections=[],onopen}: {connections?:ProviderConfig[];quotas:Quota[];now?:number;expanded?:boolean;compact?:boolean;onopen?:()=>void} = $props();
</script>
<div class={compact?'quota-strip':'quota-grid'} aria-label="서비스 사용량">
 {#each quotas as quota(quota.id)}
 {@const label=connections.find(p=>p.id===quota.provider_id)?.name??providers[quota.provider]}{@const window=quota.windows.find(w=>w.remaining_percent!==null)??quota.windows[0]}{@const remaining=window?.remaining_percent}
 {@const stale=quota.observed_at!==null && now-quota.observed_at>300000}
 {@const summary=quota.status==='unlimited'?'무제한 · 로컬':remaining!==null&&remaining!==undefined?Math.round(remaining)+'% 남음':'확인 불가'}
 {#if compact}
 <button class="quota-pill" onclick={onopen} aria-label={label+' · '+summary+(quota.status==='error'?' · 연결 실패':stale?' · 오래된 값':'')+' · 사용량 보기'} title={label+(quota.model?' / '+quota.model:'')}>
  <strong>{label}</strong><span class="quota-pill-value">{summary}</span>
  {#if quota.status==='error'}<span class="badge warn">연결 실패</span>{:else if stale}<span class="badge warn">오래된 값</span>{/if}
 </button>
 {:else}
 <article class="quota-card">
  <div class="row"><strong>{label}{quota.model?' / '+quota.model:''}</strong>{#if quota.status==='error'}<span class="badge warn">연결 실패</span>{:else if stale}<span class="badge warn">오래된 값</span>{/if}</div>
  <div class="quota-value">
   {#if quota.status==='unlimited'}<b>무제한</b><span>로컬</span>
   {:else if remaining!==null&&remaining!==undefined}<b>{Math.round(remaining)}%</b><span>남음 · {window.label}</span>
   {:else}<b class="unknown">확인 불가</b>{/if}
  </div>
  {#if remaining!==null&&remaining!==undefined}<progress max="100" value={remaining} aria-label={label+' 잔여 한도'}></progress>{/if}
  <small>{quota.status==='unlimited'?quota.host_id:quota.reason??(window?window.label+' 초기화 '+dateTime(window.resets_at):'')}</small>
  {#if quota.windows.length>1}<small>{quota.windows.filter(w=>w!==window).map(w=>w.label+' '+(w.remaining_percent===null?'확인 불가':Math.round(w.remaining_percent)+'% 남음')).join(' · ')}</small>{/if}
  {#if expanded}<div class="quota-extra"><span>{quota.account} · {quota.host_id}</span><span>확인 {age(quota.observed_at,now)}</span></div>{/if}
 </article>
 {/if}
 {/each}
</div>
