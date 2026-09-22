import type { Submission } from './types';
export type Draft = { text:string; pending:Submission|null };
export const draftKey=(server:string,project:string,work:string,run:string) => 'bibi:draft:'+JSON.stringify([server,project,work,run]);
export function loadDraft(key:string, storage:Pick<Storage,'getItem'|'setItem'>=localStorage):Draft {
  try { const d=JSON.parse(storage.getItem(key)??'null'); return d && typeof d.text==='string' ? {text:d.text,pending:d.pending??null} : {text:'',pending:null}; }
  catch {return {text:'',pending:null};}
}
export function saveDraft(key:string,draft:Draft,storage:Pick<Storage,'getItem'|'setItem'>=localStorage) {
  storage.setItem(key,JSON.stringify(draft));
}

export function submissionId(source: Pick<Crypto,'getRandomValues'> = crypto): string {
  const bytes=source.getRandomValues(new Uint8Array(16));
  return 'submission_'+Array.from(bytes,b=>b.toString(16).padStart(2,'0')).join('');
}
