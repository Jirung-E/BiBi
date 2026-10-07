<script lang="ts">
import {scrollbars} from '../scrollbars';
import type {Approval,Run} from '../types';
import {approvalLabel} from '../permissions';
let {approval,run,onrespond}: {approval:Approval;run?:Run;onrespond:(id:string,value:unknown)=>Promise<void>} = $props();
let busy=$state(false),error=$state(''),answers=$state<Record<string,string>>({});
type Question={id:string;question:string;header?:string;options?:{label:string;description:string}[]};
const input=$derived(approval.detail.input as Record<string,unknown>|undefined);
const target=$derived(approval.detail.command??input?.command??input?.file_path??input?.notebook_path);
const reason=$derived(approval.detail.reason??approval.detail.decision_reason);
const description=$derived(approval.detail.description);
const questions=$derived((approval.detail.questions??[]) as Question[]);
async function submit(value:unknown){busy=true;error='';try{await onrespond(approval.id,value);}catch(e){error=String(e);}finally{busy=false;}}
</script>
<section class="approval">
 <div class="row"><strong>{approval.title}</strong>{#if run&&approval.kind==='claude/requestApproval'}<span class="badge" title="이 요청을 시작할 때 선택한 승인 모드">{approvalLabel(run.approval_mode,run.provider)}</span>{/if}</div>
 {#if typeof description==='string'}<p>{description}</p>{/if}
 {#if typeof target==='string'}<pre use:scrollbars>{target}</pre>{/if}
 {#if typeof reason==='string'&&reason!==description}<p>{reason}</p>{/if}
 {#if approval.kind.includes('requestUserInput')}
  <form onsubmit={(e)=>{e.preventDefault();void submit({answers:Object.fromEntries(questions.map(q=>[q.id,{answers:[answers[q.id]??'']}]))});}}>
   {#each questions as q}<label>{q.question}<input bind:value={answers[q.id]} required list={'options-'+q.id} />{#if q.options}<datalist id={'options-'+q.id}>{#each q.options as o}<option value={o.label}>{o.description}</option>{/each}</datalist>{/if}</label>{/each}
   <div class="form-actions"><button class="primary" disabled={busy}>응답</button></div>
  </form>
 {:else if approval.kind.includes('requestApproval')}
  <details><summary>요청 내용</summary><pre use:scrollbars>{JSON.stringify(approval.detail,null,2)}</pre></details>
  <div class="form-actions"><button disabled={busy} onclick={()=>submit({decision:'decline'})}>거절</button><button class="primary" disabled={busy} onclick={()=>submit({decision:'accept'})}>이번 요청 승인</button></div>
 {:else}
  <p class="warning">이 요청 형식의 응답은 지원하지 않습니다.</p>
 {/if}
 {#if error}<p class="error">{error}</p>{/if}
</section>
