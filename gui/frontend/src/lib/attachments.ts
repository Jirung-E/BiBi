import type {Attachment,Provider} from './types';
import {request} from './api';
export const maxFile=8*1024*1024,maxTotal=16*1024*1024,maxFiles=8;
export const fileSize=(bytes:number)=>bytes<1024?`${bytes} B`:bytes<1024*1024?`${(bytes/1024).toFixed(1)} KiB`:`${(bytes/1024/1024).toFixed(1)} MiB`;
export function attachmentProblem(files:Attachment[],provider:Provider|undefined,host='local',readOnly=false,mode='continue'):string {
 if(!files.length)return '';
 if(host!=='local')return '첨부는 실행 호스트의 BiBi에 직접 접속해서 보내세요.';
 if(provider==='command')return '일반 실행 명령 제공자는 첨부를 지원하지 않습니다.';
 if(mode==='steer'&&provider!=='codex')return '첨부는 현재 응답이 끝난 뒤 보낼 수 있습니다.';
 if(files.length>maxFiles||files.reduce((n,a)=>n+a.size,0)>maxTotal)return '한 번에 최대 8개 · 합계 16 MiB까지 첨부할 수 있습니다.';
 for(const a of files){
  if(provider==='ollama'&&!['text','image'].includes(a.kind))return `${a.name}: Ollama에는 이미지·텍스트 파일을 첨부하세요.`;
  if(provider==='open_ai'&&!['text','image','pdf'].includes(a.kind))return `${a.name}: 이 API 연결에는 이미지·텍스트·PDF를 첨부하세요.`;
  if(provider==='claude'&&a.kind==='image'&&a.size>5*1024*1024)return `${a.name}: Claude 이미지는 5 MiB 이하여야 합니다.`;
  if(provider==='claude'&&readOnly&&a.kind==='file')return `${a.name}: 읽기 전용 Claude에는 이미지·텍스트·PDF를 첨부하세요.`;
 }
 return '';
}
export async function encodeFile(file:File):Promise<string>{
 const bytes=new Uint8Array(await file.arrayBuffer());let binary='';
 for(let i=0;i<bytes.length;i+=0x8000)binary+=String.fromCharCode(...bytes.subarray(i,i+0x8000));
 return btoa(binary);
}
export type AttachmentData={attachment:Attachment;data_base64:string};
export const readAttachment=(a:Attachment)=>request<AttachmentData>(`/api/attachments/${encodeURIComponent(a.id)}?project_key=${encodeURIComponent(a.project_key)}`);
export const imageData=(a:Attachment,data:string)=>a.kind==='image'&&['image/png','image/jpeg','image/gif','image/webp'].includes(a.media_type)?`data:${a.media_type};base64,${data}`:'';
export async function downloadAttachment(a:Attachment){
 const value=await readAttachment(a),bytes=Uint8Array.from(atob(value.data_base64),c=>c.charCodeAt(0));
 const url=URL.createObjectURL(new Blob([bytes],{type:'application/octet-stream'}));
 const link=document.createElement('a');link.href=url;link.download=a.name;document.body.append(link);link.click();link.remove();
 setTimeout(()=>URL.revokeObjectURL(url),30_000);
}
