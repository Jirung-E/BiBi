<script lang="ts">
import {onDestroy} from 'svelte';
import {command} from '../api';
import {reveal} from '../motion';
import type {Run} from '../types';
let {run,onchanged}:{run:Run;onchanged:()=>Promise<void>}=$props();
let checking=$state(false),error=$state(''),proof=$state<{native_id:string;requires_confirmation:boolean}|null>(null),confirmed=$state(false);
let alive=true;
onDestroy(()=>{alive=false;});
async function transfer(native:string,stopped:boolean){
 await command({type:'resume_external',run_id:run.id,expected_native_id:native,confirmed_stopped:stopped});
 if(alive)await onchanged();
}
async function check(){
 checking=true;error='';proof=null;confirmed=false;
 try{
  const value=await command<{native_id:string;requires_confirmation:boolean}>({type:'check_external_resume',run_id:run.id});
  if(!alive)return;
  if(value.requires_confirmation)proof=value;else await transfer(value.native_id,false);
 }catch(e){if(alive)error=e instanceof Error?e.message:String(e);}
 finally{if(alive)checking=false;}
}
async function resume(){
 if(!proof||!confirmed)return;
 checking=true;error='';
 try{await transfer(proof.native_id,true);}
 catch(e){if(alive){error=e instanceof Error?e.message:String(e);proof=null;confirmed=false;}}
 finally{if(alive)checking=false;}
}
</script>
<div class="external-resume" aria-label="외부 세션 이어받기" aria-busy={checking}>
 {#if proof}<div transition:reveal class="resume-confirm">
 <p class="muted">원래 대화 ID로 이어갑니다. 원래 앱에서는 이 세션을 종료해 주세요.</p>
 <label class="check"><input type="checkbox" bind:checked={confirmed} disabled={checking} />원래 앱에서 이 세션을 종료했습니다</label>
 <div class="row"><button type="button" class="primary" disabled={!confirmed||checking} onclick={resume}>{checking?'연결 확인 중…':'이어받기'}</button><button type="button" disabled={checking} onclick={()=>{proof=null;confirmed=false;}}>취소</button></div>
 </div>{:else}<button type="button" class="primary" disabled={checking} onclick={check}>{checking?'연결 확인 중…':'BiBi에서 이어받기'}</button>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>
<style>
 .external-resume,.resume-confirm{display:flex;flex-direction:column;align-items:flex-start;gap:.5rem;min-width:0}
 .external-resume p{margin:0;font-size:.85rem;overflow-wrap:anywhere}
 .resume-confirm .check{font-size:.9rem;white-space:normal}
</style>
