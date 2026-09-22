<script lang="ts">
import type {Quota} from '../types';
import {providers,age,dateTime} from '../format';
let {quotas,now=Date.now(),expanded=false}: {quotas:Quota[];now?:number;expanded?:boolean} = $props();
</script>
<div class="quota-grid">
 {#each quotas as quota(quota.id)}
 {@const window=quota.windows[0]}{@const remaining=window?.remaining_percent}
 {@const stale=quota.observed_at!==null && now-quota.observed_at>300000}
 <article class="quota-card">
  <div class="row"><strong>{providers[quota.provider]}{quota.model?' / '+quota.model:''}</strong>{#if quota.status==='error'}<span class="badge warn">연결 실패</span>{:else if stale}<span class="badge warn">오래된 값</span>{/if}</div>
  <div class="quota-value">
   {#if quota.status==='unlimited'}<b>무제한</b><span>로컬</span>
   {:else if remaining!==null&&remaining!==undefined}<b>{Math.round(remaining)}%</b><span>남음 · {window.label}</span>
   {:else}<b class="unknown">확인 불가</b>{/if}
  </div>
  {#if remaining!==null&&remaining!==undefined}<progress max="100" value={remaining} aria-label={providers[quota.provider]+' 잔여 한도'}></progress>{/if}
  <small>{quota.status==='unlimited'?quota.host_id:quota.reason??(window?window.label+' 초기화 '+dateTime(window.resets_at):'')}</small>
  {#if quota.windows.length>1}<small>{quota.windows.slice(1).map(w=>w.label+' '+(w.remaining_percent===null?'확인 불가':Math.round(w.remaining_percent)+'% 남음')).join(' · ')}</small>{/if}
  {#if expanded}<div class="quota-extra"><span>{quota.account} · {quota.host_id}</span><span>확인 {age(quota.observed_at,now)}</span></div>{/if}
 </article>
 {/each}
</div>
