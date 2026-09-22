<script lang="ts">
import type {Approval} from '../types';
let {approval,onrespond}: {approval:Approval;onrespond:(id:string,value:unknown)=>Promise<void>} = $props();
let busy=$state(false),error=$state(''),answers=$state<Record<string,string>>({});
type Question={id:string;question:string;header?:string;options?:{label:string;description:string}[]};
const questions=$derived((approval.detail.questions??[]) as Question[]);
async function submit(value:unknown){busy=true;error='';try{await onrespond(approval.id,value);}catch(e){error=String(e);}finally{busy=false;}}
</script>
<section class="approval">
 <strong>{approval.title}</strong>
 {#if typeof approval.detail.command==='string'}<pre>{approval.detail.command}</pre>{/if}
 {#if typeof approval.detail.reason==='string'}<p>{approval.detail.reason}</p>{/if}
 {#if approval.kind.includes('requestUserInput')}
  <form onsubmit={(e)=>{e.preventDefault();void submit({answers:Object.fromEntries(questions.map(q=>[q.id,{answers:[answers[q.id]??'']}]))});}}>
   {#each questions as q}<label>{q.question}<input bind:value={answers[q.id]} required list={'options-'+q.id} />{#if q.options}<datalist id={'options-'+q.id}>{#each q.options as o}<option value={o.label}>{o.description}</option>{/each}</datalist>{/if}</label>{/each}
   <button class="primary" disabled={busy}>응답</button>
  </form>
 {:else if approval.kind.includes('requestApproval')}
  <details><summary>요청 내용</summary><pre>{JSON.stringify(approval.detail,null,2)}</pre></details>
  <div class="row"><button disabled={busy} onclick={()=>submit({decision:'decline'})}>거절</button><button class="primary" disabled={busy} onclick={()=>submit({decision:'accept'})}>이번 요청 승인</button></div>
 {:else}
  <p class="warning">이 요청 형식의 응답은 지원하지 않습니다.</p>
 {/if}
 {#if error}<p class="error">{error}</p>{/if}
</section>
