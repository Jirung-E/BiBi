<script lang="ts">
import {onDestroy} from 'svelte';
import type {Attachment} from '../types';
import {request} from '../api';
import {submissionId} from '../drafts';
import {encodeFile,fileSize,maxFile,maxFiles,maxTotal} from '../attachments';
import Icon from './Icon.svelte';
import AttachmentCard from './AttachmentCard.svelte';
import {scrollbars} from '../scrollbars';
let {files,projectKey,draftKey,disabled=false,onadd,onremove,onbusy}:{files:Attachment[];projectKey:string;draftKey:string;disabled?:boolean;onadd:(a:Attachment,key:string)=>void;onremove:(id:string)=>void;onbusy:(value:boolean)=>void}=$props();
type Upload={id:string;file:File;busy:boolean;error:string};
let picker=$state<HTMLInputElement>(),queue=$state<Upload[]>([]),error=$state('');
let disposed=false;
const count=$derived(files.length+queue.length);
$effect(()=>onbusy(queue.length>0));
onDestroy(()=>{disposed=true;onbusy(false);});
export function choose(){picker?.click();}
export function paste(event:ClipboardEvent){
 const files=Array.from(event.clipboardData?.files??[]);if(!files.length)return;
 event.preventDefault();void add(files);
}
export function drop(event:DragEvent){
 if(!event.dataTransfer?.types.includes('Files'))return;
 event.preventDefault();void add(Array.from(event.dataTransfer.files));
}
async function upload(item:Upload){
 if(item.busy)return;
 const key=draftKey,project=projectKey;item.busy=true;item.error='';
 try{
  const data=await encodeFile(item.file);
  const result=await request<Attachment>('/api/attachments','POST',{id:item.id,project_key:project,name:item.file.name,data_base64:data});
  onadd(result,key);
  if(!disposed)queue=queue.filter(i=>i.id!==item.id);
 }catch(e){if(!disposed){item.busy=false;item.error=e instanceof Error?e.message:String(e);}}
}
async function add(selected:File[]){
 if(disabled||disposed)return;error='';
 if(count+selected.length>maxFiles){error='파일은 최대 8개까지 첨부할 수 있습니다.';return;}
 if(selected.some(f=>f.size>maxFile)){error='파일당 최대 8 MiB까지 첨부할 수 있습니다.';return;}
 if([...files,...queue.map(i=>i.file),...selected].reduce((n,f)=>n+f.size,0)>maxTotal){error='첨부 파일 합계는 16 MiB 이하여야 합니다.';return;}
 const added=selected.map(file=>({id:'att_'+submissionId().slice('submission_'.length),file,busy:false,error:''}));
 queue=[...queue,...added];
 // Encode each selection sequentially; total pending bytes are bounded above.
 for(const item of added){if(disposed)break;await upload(queue.find(i=>i.id===item.id)!);}
}
</script>
<input bind:this={picker} type="file" multiple hidden aria-label="첨부 파일 선택" onchange={e=>{void add(Array.from(e.currentTarget.files??[]));e.currentTarget.value='';}} />
{#if files.length||queue.length}
 <div class="draft-attachments" use:scrollbars aria-label="첨부 파일">
  {#each files as file(file.id)}<div class="draft-file"><AttachmentCard attachment={file} compact /><button class="icon-button" type="button" aria-label={file.name+' 첨부 제거'} title="첨부 제거" {disabled} onclick={()=>onremove(file.id)}><Icon name="close" /></button></div>{/each}
  {#each queue as item(item.id)}<div class="upload-file"><Icon name="attachment" /><span class="upload-name">{item.file.name}<small>{fileSize(item.file.size)} · {item.busy?'업로드 중':'업로드 실패'}</small></span>{#if item.error}<button type="button" onclick={()=>void upload(item)}>재시도</button><button class="icon-button" type="button" aria-label={item.file.name+' 첨부 제거'} onclick={()=>queue=queue.filter(i=>i.id!==item.id)}><Icon name="close" /></button>{/if}{#if item.error}<span class="error" role="alert">{item.error}</span>{/if}</div>{/each}
 </div>
{/if}
{#if error}<p class="error" role="alert">{error}</p>{/if}
<style>
 .draft-attachments{display:flex;gap:.5rem;overflow:auto;max-height:10rem;flex-shrink:0;padding:.2rem;align-items:flex-start}
 .draft-file{display:flex;align-items:center;gap:.2rem;max-width:100%;flex-shrink:0}
 .upload-file{display:flex;align-items:center;gap:.5rem;flex-wrap:wrap;max-width:20rem;border:1px solid var(--border);border-radius:.7rem;padding:.5rem}
 .upload-name{min-width:0;overflow-wrap:anywhere;flex:1}
 small{display:block;color:var(--muted)}
</style>
