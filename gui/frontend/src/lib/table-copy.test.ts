// @vitest-environment jsdom
import {it,expect,vi,afterEach} from 'vitest';
import {tableClipboard,copyTable} from './table-copy';
function table(html:string){document.body.innerHTML=html;return document.querySelector('table')!;}
afterEach(()=>{vi.unstubAllGlobals();Reflect.deleteProperty(document,'execCommand');document.body.innerHTML='';});
it('copies table headers, formatted cells and quoted multiline spreadsheet cells',()=>{
 const value=tableClipboard(table('<table><thead><tr><th>항목</th><th>값</th></tr></thead><tbody><tr><td><strong>하나</strong></td><td>1</td></tr><tr><td>"둘"</td><td>A<br>B</td></tr><tr><td>끝</td><td>A\tB</td></tr></tbody></table>'));
 expect(value.text).toBe('항목\t값\n하나\t1\n"""둘"""\t"A\nB"\n끝\t"A\tB"');expect(value.html).toContain('<strong>하나</strong>');
});
it('strips interactive elements, URLs, attributes and styling from copied HTML',()=>{
 const value=tableClipboard(table('<table style="color:red" data-secret="x"><tr><td onclick="alert(1)"><a href="https://example.com">링크</a><img src="https://example.com/image"><em>강조</em></td></tr></table>'));
 expect(value.html).toBe('<table><tbody><tr><td>링크<em>강조</em></td></tr></tbody></table>');expect(value.text).toBe('링크강조');
});
it('uses the rich clipboard when available without changing input focus or selection',async()=>{
 const t=table('<table><tr><td>value</td></tr></table>'),input=document.createElement('textarea');document.body.append(input);input.value='draft';input.focus();input.setSelectionRange(1,3);
 const items:Record<string,Blob>[]=[];vi.stubGlobal('ClipboardItem',class{constructor(value:Record<string,Blob>){items.push(value);}});const write=vi.fn().mockResolvedValue(undefined);vi.stubGlobal('navigator',{clipboard:{write}});
 await copyTable(t);expect(write).toHaveBeenCalledOnce();expect(Object.keys(items[0])).toEqual(['text/plain','text/html']);expect(items[0]['text/html'].type).toBe('text/html');expect(document.activeElement).toBe(input);expect([input.selectionStart,input.selectionEnd]).toEqual([1,3]);
});
it('supplies both formats on HTTP copy and restores the focused draft and selection',async()=>{
 const t=table('<table><tr><td>value</td></tr></table>'),input=document.createElement('textarea');document.body.append(input);input.value='draft';input.focus();input.setSelectionRange(1,3);vi.stubGlobal('navigator',{clipboard:undefined});
 const data:Record<string,string>={};Object.defineProperty(document,'execCommand',{configurable:true,value:(command:string)=>{expect(command).toBe('copy');const event=new Event('copy',{cancelable:true});Object.defineProperty(event,'clipboardData',{value:{setData:(key:string,value:string)=>data[key]=value}});document.dispatchEvent(event);return event.defaultPrevented;}});
 await copyTable(t);expect(data).toEqual({'text/plain':'value','text/html':'<table><tbody><tr><td>value</td></tr></tbody></table>'});expect(document.activeElement).toBe(input);expect([input.selectionStart,input.selectionEnd]).toEqual([1,3]);expect(document.querySelectorAll('textarea')).toHaveLength(1);
});
it('falls back to text when rich and legacy clipboard writes fail',async()=>{
 const t=table('<table><tr><td>value</td></tr></table>');vi.stubGlobal('ClipboardItem',class{});const write=vi.fn().mockRejectedValue(new Error('denied')),writeText=vi.fn().mockResolvedValue(undefined);vi.stubGlobal('navigator',{clipboard:{write,writeText}});Object.defineProperty(document,'execCommand',{configurable:true,value:()=>false});
 await copyTable(t);expect(write).toHaveBeenCalledOnce();expect(writeText).toHaveBeenCalledWith('value');
});
it('reports a real failure when every clipboard method is unavailable',async()=>{
 const t=table('<table><tr><td>value</td></tr></table>');vi.stubGlobal('navigator',{clipboard:undefined});Object.defineProperty(document,'execCommand',{configurable:true,value:()=>false});await expect(copyTable(t)).rejects.toThrow('표를 복사하지 못했습니다.');expect(document.querySelector('textarea')).toBeNull();
});
