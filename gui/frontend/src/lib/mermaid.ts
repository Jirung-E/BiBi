import DOMPurify from 'dompurify';

export type Diagram = {url:string;width:number;height:number};
export type DiagramPalette = {text:string;surface:string;soft:string;border:string;muted:string;dark:boolean};
const MAX_SOURCE=20_000, MAX_SVG=1_000_000, MAX_CACHE=4_000_000;
const cache=new Map<string,Diagram>();
let cacheSize=0, serial=0, queue:Promise<unknown>=Promise.resolve();
let library:Promise<typeof import('mermaid')>|undefined;

// Rendering measures an SVG in the document. Reject resource-loading syntax
// before Mermaid sees it; strict mode alone only disables HTML/click handlers.
export function diagramProblem(source:string):string|null {
 if(source.length>MAX_SOURCE||source.split('\n').length>400)return '다이어그램이 너무 큽니다. 코드로 확인하세요.';
 if(!source.trim())return '다이어그램 코드가 아직 없습니다.';
 // Canonicalize CSS escapes/comments before checking resource functions.
 const checked=source.replace(/\/\*[\s\S]*?\*\//g,'').replace(/\\([0-9a-f]{1,6})\s?/gi,(_,hex:string)=>String.fromCodePoint(Math.min(parseInt(hex,16)||0xfffd,0x10ffff))).replace(/\\(.)/g,'$1');
 if(/%%\s*\{|^\s*---|\burl\s*\(|@import\b|["']?\bimg["']?\s*:|<\s*\/?\s*[a-z][\s\S]*?>/im.test(checked))
  return '설정 지시문·HTML·외부 이미지가 포함된 다이어그램은 코드로 확인하세요.';
 return null;
}
export function diagramPalette(element:HTMLElement):DiagramPalette {
 const css=getComputedStyle(element),color=(name:string)=>css.getPropertyValue(name).trim();
 return {text:color('--text'),surface:color('--surface'),soft:color('--accent-soft'),border:color('--border'),muted:color('--muted'),dark:css.colorScheme==='dark'};
}
const keyFor=(source:string,palette:DiagramPalette)=>JSON.stringify([source,palette]);
export function cachedDiagram(source:string,palette:DiagramPalette){return cache.get(keyFor(source,palette));}

export function renderDiagram(source:string,palette:DiagramPalette,signal:AbortSignal):Promise<Diagram|undefined> {
 const problem=diagramProblem(source);
 if(problem)return Promise.reject(new Error(problem));
 const key=keyFor(source,palette);
 const work=async()=>{
  if(signal.aborted)return;
  const previous=cache.get(key);
  if(previous){cache.delete(key);cache.set(key,previous);return previous;}
  const {default:mermaid}=await(library??=import('mermaid').catch(error=>{library=undefined;throw error;}));
  if(signal.aborted)return;
  mermaid.initialize({
   startOnLoad:false,securityLevel:'strict',suppressErrorRendering:true,
   maxTextSize:MAX_SOURCE,maxEdges:180,htmlLabels:false,
   flowchart:{htmlLabels:false,useMaxWidth:false},sequence:{useMaxWidth:false},
   theme:'base',fontFamily:'-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
   themeVariables:{
    darkMode:palette.dark,fontSize:'14px',background:palette.surface,
    primaryColor:palette.soft,primaryTextColor:palette.text,primaryBorderColor:palette.border,
    secondaryColor:palette.surface,tertiaryColor:palette.soft,
    lineColor:palette.muted,textColor:palette.text,mainBkg:palette.soft,
    nodeTextColor:palette.text,edgeLabelBackground:palette.surface,
    actorBkg:palette.soft,actorBorder:palette.border,actorTextColor:palette.text,
    signalColor:palette.muted,signalTextColor:palette.text,
    labelBoxBkgColor:palette.surface,labelBoxBorderColor:palette.border,labelTextColor:palette.text,
    noteBkgColor:palette.soft,noteTextColor:palette.text,noteBorderColor:palette.border
   }
  });
  const stage=document.createElement('div');
  stage.setAttribute('aria-hidden','true');stage.inert=true;
  Object.assign(stage.style,{position:'fixed',left:'0',top:'0',visibility:'hidden',pointerEvents:'none',contain:'layout style'});
  document.body.append(stage);
  try {
   const {svg}=await mermaid.render('bibi-mermaid-'+(++serial),source,stage);
   if(signal.aborted)return;
   if(svg.length>MAX_SVG)throw new Error('다이어그램이 너무 큽니다. 코드로 확인하세요.');
   const clean=DOMPurify.sanitize(svg,{
    USE_PROFILES:{svg:true,svgFilters:true},
    FORBID_TAGS:['foreignObject','script','image','a','animate','animateMotion','animateTransform','set'],
    ALLOW_DATA_ATTR:false
   });
   const root=new DOMParser().parseFromString(clean,'image/svg+xml').documentElement;
   const box=root.getAttribute('viewBox')?.split(/[\s,]+/).map(Number);
   if(root.localName!=='svg'||!box||box.length!==4||!box.every(Number.isFinite)||box[2]<=0||box[3]<=0)
    throw new Error('다이어그램을 표시하지 못했습니다. 코드를 확인하세요.');
   const [, ,width,height]=box;
   root.setAttribute('xmlns','http://www.w3.org/2000/svg');
   root.setAttribute('width',String(width));root.setAttribute('height',String(height));
   // An SVG image cannot run scripts, attach handlers, or style the app.
   const result={url:'data:image/svg+xml;charset=utf-8,'+encodeURIComponent(new XMLSerializer().serializeToString(root)),width,height};
   cache.set(key,result);cacheSize+=key.length+result.url.length;
   while(cache.size>48||cacheSize>MAX_CACHE){
    const first=cache.entries().next().value!;
    cacheSize-=first[0].length+first[1].url.length;cache.delete(first[0]);
   }
   return result;
  } finally {stage.remove();}
 };
 const result=queue.then(work,work);
 queue=result.catch(()=>undefined);
 return result;
}
