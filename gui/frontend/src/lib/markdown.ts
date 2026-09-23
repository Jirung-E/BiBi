import {marked} from 'marked';
import DOMPurify from 'dompurify';
export function markdown(text:string):string {
 return DOMPurify.sanitize(marked.parse(text,{async:false,gfm:true,breaks:false}),{
  ALLOWED_TAGS:['p','br','strong','em','del','s','blockquote','ul','ol','li','pre','code','h1','h2','h3','h4','h5','h6','hr','a','table','thead','tbody','tr','th','td','input'],
  ALLOWED_ATTR:['href','title','class','start','align','type','checked','disabled'],
  FORBID_ATTR:['style'], ALLOW_DATA_ATTR:false
 });
}
