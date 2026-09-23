<script lang="ts">
import type {Run,Work,Host} from '../types';
import {age,providerName,states,shortId} from '../format';
import SessionActions from './SessionActions.svelte';
let {run,work,host,now,onopen,onclose,onchanged}: {run:Run;work:Work|undefined;host:Host|undefined;now:number;onopen:()=>void;onclose:()=>void;onchanged:()=>Promise<void>} = $props();
const labels:Record<string,string>={open_history:'기록 열기',stream_output:'실시간 출력',send_to_active:'현재 작업 입력',respond_to_input:'요청 응답',start_fresh:'새 세션',continue_session:'대화 이어가기',interrupt:'중단',read_quota:'한도 조회'};
const stale=$derived(['running','waiting_user','waiting_expert'].includes(run.state)&&now-(host?.observed_at??run.observed_at)>30000);
</script>
<aside class="run-details card">
 <div class="row"><span class="muted">선택한 실행</span><button class="icon-button mobile-only" aria-label="상세 닫기" onclick={onclose}>×</button><span class={'badge '+run.state}>{states[run.state]}</span></div>
 <div><small>{run.role} · {providerName(run)}</small><h2>{run.title}</h2></div>
 <dl><dt>업무</dt><dd>{work?.title??shortId(run.work_id)}</dd><dt>실행</dt><dd title={run.id}>{shortId(run.id)}</dd><dt>호스트</dt><dd>{host?.name??run.host_id}</dd><dt>연결</dt><dd>{run.agent_kind==='subagent'?'런타임 서브에이전트':run.origin==='external'?'외부 세션':'BiBi 세션'}</dd></dl>
 <dl><dt>현재 모델</dt><dd>{run.model||'확인 대기'}</dd><dt>작업 경로</dt><dd>{run.workspace}</dd></dl>
 <SessionActions {run} {onchanged} />
 <hr />
 {#if run.activity}<div><small>업무 보고 · {age(run.activity.reported_at,now)}</small><p class="note">{run.activity.summary}</p>{#if run.activity.wait_reason}<p>{run.activity.wait_reason}</p>{/if}</div>{/if}
 <dl><dt>실행 관측</dt><dd>{age(run.observed_at,now)}</dd><dt>출처</dt><dd>{run.observation_source}</dd></dl>
 {#if stale}<p class="warning">호스트 관측이 오래되었습니다.</p>{/if}
 {#if run.wait_reason}<p class="warning">{run.wait_reason}</p>{/if}
 {#if run.error}<p class="error">{run.error}</p>{/if}
 <details><summary>세션 저장 위치</summary><p class="prewrap">{run.runtime?.session_file??'서버의 BiBi 데이터 폴더에 기록됨'}</p>{#if run.session_key}<label>서비스 세션 ID<input readonly value={run.session_key} onfocus={(e)=>e.currentTarget.select()} /></label>{#if run.provider==='claude'&&run.agent_kind!=='subagent'}<label>Claude에서 열기<input readonly value={'claude --resume '+run.session_key} onfocus={(e)=>e.currentTarget.select()} /></label><small>{host?.name??run.host_id}에서 실행 · 작업 경로: {run.workspace}</small>{/if}{/if}</details>
 <details><summary>지원 기능</summary>
  {#each Object.entries(run.capabilities) as [name,cap]}
   <div class="capability"><span>{labels[name]??name}</span><span>{cap.supported?'지원':'미지원'}</span>{#if !cap.supported}<small>{cap.reason}</small>{/if}</div>
  {/each}
 </details>
 <button class="primary open-run" onclick={onopen}>대화 열기 ↗</button>
</aside>
