import type {Detail,Event,Message} from './types';
// A detail snapshot and its event cursor are read in the same store transaction.
export function applyChatEvent(detail:Detail,event:Event):boolean {
 if(detail.last_seq===undefined||!['output','message'].includes(event.kind))return false;
 const d=event.data as Partial<Message>&{message_id?:string;delta?:string};
 if(d.run_id!==detail.run.id&&!detail.conversation?.some(m=>m.run_id===d.run_id))return false;
 if(event.seq<=detail.last_seq)return true;
 const messages=detail.conversation??detail.messages;
 const id=event.kind==='output'?d.message_id:d.id;if(!id||!d.run_id)return false;
 const prior=messages.find(m=>m.id===id);
 let next:Message;
 if(event.kind==='output'){
  if(typeof d.delta!=='string'||typeof d.role!=='string')return false;
  next={...(prior??{id,run_id:d.run_id,role:d.role,created_at:d.created_at??event.created_at,text:''}),text:(prior?.text??'')+d.delta};
 }else {if(typeof d.text!=='string'||typeof d.role!=='string')return false;next=d as Message;}
 const set=(list:Message[])=>{const i=list.findIndex(m=>m.id===id);if(i<0)list.push(next);else list[i]=next;};
 set(messages);if(messages!==detail.messages&&d.run_id===detail.run.id)set(detail.messages);
 detail.last_seq=event.seq;return true;
}
