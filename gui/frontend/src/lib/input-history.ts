export function inputHistory(){
 let index=-1,draft='';
 return {reset(){index=-1;draft='';},navigate(key:string,value:string,start:number,end:number,history:string[]):string|null{
  if(start!==end)return null;
  if(key==='ArrowUp'){
   if(value.slice(0,start).includes('\n')||!history.length)return null;
   if(index<0)draft=value;
   if(index+1>=history.length)return null;
   return history[++index];
  }
  if(key==='ArrowDown'&&index>=0&&!value.slice(end).includes('\n'))return --index<0?draft:history[index];
  return null;
 }};
}
