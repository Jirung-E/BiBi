import {parseDocument} from 'yaml';

type Prepared={code:string;problem:string|null;configured:boolean};
// Only the diagram body and a plain title reach Mermaid. Source-supplied
// initialization/settings never override the app's security or theme config.
export function prepareDiagram(source:string):Prepared {
 let code=source.trim(),configured=false;
 const fail=(problem:string):Prepared=>({code:'',problem,configured});
 if(source.length>20_000||source.split('\n').length>400)return fail('다이어그램이 너무 큽니다. 코드로 확인하세요.');
 if(!code)return fail('다이어그램 코드가 아직 없습니다.');
 let title='';
 if(/^---[ \t]*(?:\r?\n|$)/.test(code)){
  const header=code.match(/^---[ \t]*\r?\n([\s\S]*?)\r?\n[ \t]*---[ \t]*(?:\r?\n|$)/);
  if(!header)return fail('다이어그램 머리말이 완성되지 않았습니다. 닫는 ---를 확인하세요.');
  try{
   const document=parseDocument(header[1]);
   if(document.errors.length||document.warnings.length)throw new Error('metadata');
   const value=document.toJS({maxAliasCount:0});
   if(value&&typeof value==='object'&&typeof value.title==='string')title=value.title;
  }catch{return fail('다이어그램 제목·설정의 YAML 형식을 확인하세요.');}
  code=code.slice(header[0].length);configured=true;
 }
 code=code.replace(/%%\s*\{[\s\S]*?\}\s*%%/g,()=>{configured=true;return '';});
 if(/%%\s*\{/.test(code))return fail('다이어그램 설정 지시문이 완성되지 않았습니다. 닫는 }%%를 확인하세요.');
 // Simple text emphasis is harmless, but is rendered as SVG text rather than
 // HTML. Keep only attribute-free line breaks, which Mermaid supports in SVG.
 code=code.replace(/<\/?(?:b|strong|i|em|u|s|del|span)\s*>/gi,'');
 if(title)code='---\ntitle: '+JSON.stringify(title)+'\n---\n'+code;
 const checked=code.replace(/\/\*[\s\S]*?\*\//g,'')
  .replace(/\\([0-9a-f]{1,6})\s?/gi,(_,hex:string)=>String.fromCodePoint(Math.min(parseInt(hex,16)||0xfffd,0x10ffff)))
  .replace(/\\(.)/g,'$1');
 if(/\burl\s*\(|@import\b/i.test(checked))return fail('외부 리소스를 불러오는 CSS는 미리보기에서 지원하지 않습니다.');
 if(/["']?\bimg["']?\s*:/i.test(checked))return fail('이미지 노드는 미리보기에서 지원하지 않습니다. 코드로 확인하세요.');
 const html=checked.replace(/<br\s*\/?>/gi,'').match(/<\s*\/?\s*([a-z][\w:-]*)\b[\s\S]*?>/i);
 if(html)return fail('<'+html[1].toLowerCase()+'> HTML은 미리보기에서 지원하지 않습니다. 줄바꿈에는 <br/>를 사용할 수 있습니다.');
 return {code,problem:null,configured};
}
export const diagramProblem=(source:string)=>prepareDiagram(source).problem;
