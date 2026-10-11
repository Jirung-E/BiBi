import {marked,Renderer} from 'marked';
import DOMPurify from 'dompurify';
import type {Attachment} from './types';
export function markdown(text:string,files:Attachment[]=[]):string {
 const renderer=new Renderer();
 const registered=(href:string)=>files.find(a=>a.source===href||href===`#bibi-attachment-${a.id}`);
 const escape=(s:string)=>s.replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;');
 const link=renderer.link.bind(renderer);
 renderer.link=function(token){const a=registered(token.href);return a?`<a href="#bibi-attachment-${escape(a.id)}">${this.parser.parseInline(token.tokens)}</a>`:token.href.startsWith('#bibi-attachment-')?escape(token.text):link(token);};
 renderer.image=token=>{const a=registered(token.href);return a?`<a href="#bibi-attachment-${escape(a.id)}">${escape(token.text||a.name)}</a>`:'';};
 return DOMPurify.sanitize(marked.parse(text,{async:false,gfm:true,breaks:false,renderer}),{
  ALLOWED_TAGS:['p','br','strong','em','del','s','blockquote','ul','ol','li','pre','code','h1','h2','h3','h4','h5','h6','hr','a','table','thead','tbody','tr','th','td','input'],
  ALLOWED_ATTR:['href','title','class','start','align','type','checked','disabled'],
  FORBID_ATTR:['style'], ALLOW_DATA_ATTR:false
 });
}

// Each message owns one cache. A fresh API snapshot recreates attachment arrays
// even when the answer is unchanged; avoid reparsing a long transcript on every delta.
export function messageMarkdown(){
 let previousText:string|undefined,previousFiles='',html='';
 return (text:string,files:Attachment[]=[])=>{
  const key=JSON.stringify(files.map(({id,name,source})=>[id,name,source]));
  if(text!==previousText||key!==previousFiles){html=markdown(text,files);previousText=text;previousFiles=key;}
  return html;
 };
}
