<script lang="ts">
import type {Run} from '../types';
import {stateLabel} from '../format';
let {run}:{run:Run}=$props();
const active=$derived(run.origin==='managed'&&['queued','running','waiting_user','waiting_expert'].includes(run.state));
const animated=$derived(run.state==='running'||run.state==='waiting_expert');
</script>
{#if active}<div class="chat-progress" role="status" aria-label="에이전트 진행 상태"><span class="progress-dots" class:animated aria-hidden="true"><i></i><i></i><i></i></span><span>{run.phase==='중단 요청 중'?'중단 중…':run.state==='waiting_user'?'승인·입력 대기':run.state==='queued'?'전송 대기':stateLabel(run)}</span></div>{/if}
<style>
.chat-progress{display:flex;align-items:center;gap:.6rem;align-self:flex-start;padding:.55rem .8rem;border-radius:var(--radius);background:var(--accent-soft);color:var(--text);font-size:.85rem;min-height:2rem}
.progress-dots{display:flex;gap:.25rem;align-items:center;height:1rem}.progress-dots i{width:.3125rem;height:.3125rem;border-radius:50%;background:currentColor;opacity:.5}.animated i{animation:chat-pulse 1.2s ease-in-out infinite}.animated i:nth-child(2){animation-delay:.15s}.animated i:nth-child(3){animation-delay:.3s}@keyframes chat-pulse{0%,70%,100%{opacity:.3;transform:translateY(0)}35%{opacity:1;transform:translateY(-.1875rem)}}@media(prefers-reduced-motion:reduce){.animated i{animation:none;opacity:.7}}
</style>
