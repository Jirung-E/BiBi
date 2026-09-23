export type Navigation = {view:'canvas'|'conversation'|'usage';project:string;run:string;modal:''|'project'|'new'|'settings'|'context'|'host'};
export function readNavigation(url:URL):Navigation {
 const q=url.searchParams,view=q.get('view'),modal=q.get('modal');
 return {view:view==='conversation'||view==='usage'?view:'canvas',project:q.get('project')??'',run:q.get('run')??'',modal:['project','new','settings','context','host'].includes(modal??'')?modal as Navigation['modal']:''};
}
export function navigationUrl(url:URL,state:Navigation):URL {
 const next=new URL(url);for(const [key,value] of Object.entries(state)){if(value)next.searchParams.set(key,value);else next.searchParams.delete(key);}return next;
}
