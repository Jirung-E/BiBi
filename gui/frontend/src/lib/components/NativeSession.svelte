<script lang="ts">
import TerminalAction from './TerminalAction.svelte';
import type {Run} from '../types';import {command} from '../api';import Icon from './Icon.svelte';import {surfaceFade} from '../motion';import {modalDialog} from '../modal';import {scrollbars} from '../scrollbars';
let {catalogActions=false,run,onchanged}:{catalogActions?:boolean;run:Run;onchanged:()=>Promise<void>}=$props();
let expanded=$state(false),busy=$state(false),error=$state(''),notice=$state('');
let target=$state<{native_id:string;workspace:string;config_directory:string;command:string}|null>(null);
const available=$derived(run.host_id==='local'&&(run.provider==='claude'||catalogActions&&run.provider==='codex')&&run.agent_kind!=='subagent'&&!!run.session_key);
const canRelease=$derived(['completed','interrupted','failed'].includes(run.state));
async function act(){busy=true;error='';notice='';try{if(run.origin==='external'){const result=await command<{message:string}>({type:'open_session_window',run_id:run.id});notice=result.message;}else{target=await command({type:'release_claude',run_id:run.id,expected_native_id:run.session_key});await onchanged();}}catch(e){error=e instanceof Error?e.message:String(e);}finally{busy=false;}}
</script>
{#if available}<div class="native-session"><button class="icon-button" aria-label="원래 터미널" title="원래 터미널" aria-expanded={expanded} onclick={()=>expanded=!expanded}><Icon name="terminal" /></button>
{#if expanded}<dialog class="native-session-panel card" use:modalDialog use:scrollbars transition:surfaceFade aria-label="원래 터미널" oncancel={e=>{e.preventDefault();expanded=false;}}><div class="row"><strong>터미널</strong><button class="icon-button" aria-label="터미널 메뉴 닫기" onclick={()=>expanded=false}><Icon name="close" /></button></div>
{#if catalogActions}<TerminalAction id={'run:'+run.id} {onchanged} />
{:else if run.origin==='external'}<button disabled={busy} onclick={act}>{busy?'창 확인 중…':'원래 창 열기'}</button><small>이 세션이 실행 중인 호스트의 창을 표시합니다.</small>
{:else if !target}<p>BiBi의 대기 프로세스를 종료하고 같은 대화를 CLI에서 이어갑니다.</p><button disabled={busy||!canRelease} onclick={act}>{busy?'실행 반환 중…':'터미널로 돌려주기'}</button>{#if !canRelease}<small>현재 응답과 대기 메시지를 먼저 정리하세요.</small>{/if}
{:else}<p>프로세스를 정리했습니다. 아래 작업 폴더에서 실행하세요.</p><label>작업 폴더<input readonly value={target.workspace} onfocus={e=>e.currentTarget.select()} /></label><label>실행 명령<input readonly value={target.command} onfocus={e=>e.currentTarget.select()} /></label><details><summary>Claude 설정 위치</summary><input readonly aria-label="Claude 설정 위치" value={target.config_directory} onfocus={e=>e.currentTarget.select()} /><small>기존 터미널도 같은 CLAUDE_CONFIG_DIR을 사용해야 합니다.</small></details>{/if}
{#if notice}<p role="status">{notice}</p>{/if}{#if error}<p class="error" role="alert">{error}</p>{/if}</dialog>{/if}</div>{/if}
<style>
.native-session{position:relative;display:flex}.native-session-panel{position:fixed;margin:auto;max-height:80dvh;overflow:auto;width:min(22rem,calc(100vw - 3rem));display:flex;flex-direction:column;gap:.6rem;padding:.8rem;border:1px solid var(--border);border-radius:var(--radius);background:var(--surface);box-shadow:0 1rem 4rem var(--modal-shadow);font-size:.9rem}.native-session-panel p{margin:0}.native-session-panel input{min-width:0;width:100%}.native-session-panel .row{justify-content:space-between}.native-session-panel small{color:var(--muted)}
.native-session-panel::backdrop{background:var(--backdrop)}
</style>
