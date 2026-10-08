<script lang="ts">
import {onDestroy} from 'svelte';
import type {Attachment} from '../types';
import {downloadAttachment,fileSize,imageData,readAttachment} from '../attachments';
import {modalDialog} from '../modal';
import {scrollbars} from '../scrollbars';
import Icon from './Icon.svelte';
let {attachment,compact=false}:{attachment:Attachment;compact?:boolean}=$props();
let image=$state(''),error=$state(''),loading=$state(false),preview=$state(false);
let disposed=false,version=0;
onDestroy(()=>{disposed=true;version++;});
async function load(){
 if(image||loading||attachment.kind!=='image')return;
 loading=true;error='';const revision=++version;
 try{const data=await readAttachment(attachment);if(!disposed&&revision===version)image=imageData(data.attachment,data.data_base64);}
 catch(e){if(!disposed&&revision===version)error=e instanceof Error?e.message:String(e);}
 finally{if(!disposed&&revision===version)loading=false;}
}
function visible(node:HTMLElement){
 const observer=new IntersectionObserver(entries=>{if(entries.some(e=>e.isIntersecting))void load();else if(!preview){version++;image='';loading=false;}},{rootMargin:'150px'});
 observer.observe(node);return {destroy:()=>observer.disconnect()};
}
async function download(){try{error='';await downloadAttachment(attachment);}catch(e){error=e instanceof Error?e.message:String(e);}}
async function open(){if(attachment.kind==='image'){preview=true;await load();}else await download();}
</script>
<div class="attachment" class:compact>
 <button type="button" class="attachment-card" use:visible onclick={()=>void open()} title={attachment.name+' · '+fileSize(attachment.size)} aria-label={attachment.name+(attachment.kind==='image'?' 미리보기':' 다운로드')}>
  <span class="attachment-thumbnail">{#if image}<img src={image} alt="" onerror={()=>{image='';error='이미지를 표시할 수 없습니다. 원본 파일을 다운로드하세요.';}} />{:else}<Icon name="attachment" />{/if}</span>
  <span class="attachment-label">{attachment.name}<small>{fileSize(attachment.size)}{loading?' · 불러오는 중':''}</small></span>
 </button>
 {#if error&&!preview}<span class="error" role="status">{error}</span>{/if}
</div>
{#if preview}<dialog use:modalDialog class="attachment-preview" aria-label={attachment.name+' 미리보기'} oncancel={()=>preview=false}>
 <header><strong>{attachment.name}</strong><button class="icon-button" type="button" aria-label="첨부 다운로드" title="다운로드" onclick={()=>void download()}><Icon name="import" /></button><button class="icon-button" type="button" aria-label="첨부 미리보기 닫기" onclick={()=>preview=false}><Icon name="close" /></button></header>
 <div class="preview-body" use:scrollbars>{#if image}<img src={image} alt={attachment.name} />{:else if loading}<span role="status">불러오는 중…</span>{/if}{#if error}<p class="error" role="alert">{error}</p>{/if}</div>
</dialog>{/if}
<style>
 .attachment{max-width:100%;min-width:0}
 .attachment-card{display:flex;align-items:center;gap:.65rem;text-align:left;max-width:100%;padding:.5rem .75rem;border-radius:.7rem;height:auto}
 .attachment-thumbnail{width:3.5rem;height:3.5rem;display:grid;place-items:center;flex-shrink:0;background:var(--neutral-soft);border-radius:.4rem}
 .attachment-thumbnail img{width:100%;height:100%;object-fit:contain}
 .attachment-label{min-width:0;max-width:18rem;overflow-wrap:anywhere}
 small{display:block;color:var(--muted);font-size:.8rem;font-weight:400}
 .compact .attachment-thumbnail{width:2.4rem;height:2.4rem}
 .compact .attachment-label{max-width:10rem}
 .attachment-preview{padding:1rem;width:min(56rem,calc(100% - 2rem));max-height:calc(100dvh - 2rem);border:1px solid var(--border);border-radius:1rem;background:var(--surface);color:var(--text)}
 header{display:flex;align-items:center;gap:.5rem;margin-bottom:1rem}
 header strong{flex:1;min-width:0;overflow-wrap:anywhere}
 .preview-body{display:grid;place-items:center;overflow:auto;max-height:calc(100dvh - 9rem);min-height:6rem}
 .preview-body img{max-width:100%;max-height:calc(100dvh - 9rem);object-fit:contain}
</style>
