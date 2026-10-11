// @vitest-environment jsdom
import {it,expect,vi} from 'vitest';
import {marked} from 'marked';
import {markdown,messageMarkdown} from './markdown';
it('renders headings, code, lists and GFM tables while removing active HTML and URLs',()=>{
 const html=markdown('# 제목\n\n**굵게**\n\n- 하나\n- 둘\n\n```rust\nfn main() {}\n```\n\n| 열 | 값 |\n|---|---|\n| a | b |\n\n<script>alert(1)</script><img src=x onerror=alert(1)>\n\n[위험](javascript:alert(1)) <a href="https://example.com" onclick="alert(1)">안전한 링크</a>');
 const node=document.createElement('div');node.innerHTML=html;
 expect(node.querySelector('h1')?.textContent).toBe('제목');expect(node.querySelectorAll('li')).toHaveLength(2);expect(node.querySelector('pre code')?.textContent).toContain('fn main');expect(node.querySelector('table')).not.toBeNull();
 expect(node.querySelector('script,img,[onclick],[onerror]')).toBeNull();expect([...node.querySelectorAll('a[href]')].some(a=>a.getAttribute('href')?.startsWith('javascript:'))).toBe(false);expect(node.querySelector('a[href]')?.getAttribute('href')).toBe('https://example.com');
});

it('maps only registered result links and images to attachment actions',()=>{
 const a={id:'att_'+'a'.repeat(32),project_key:'p',name:'out.png',kind:'image' as const,media_type:'image/png',size:1,sha256:'fixture',source:'outputs/out.png'};
 const node=document.createElement('div');node.innerHTML=markdown('![out](outputs/out.png) [download](#bibi-attachment-'+a.id+') ![remote](https://example.com/track.png) [unknown](#bibi-attachment-att_'+'b'.repeat(32)+')',[a]);
 expect(node.querySelectorAll('a')).toHaveLength(2);expect([...node.querySelectorAll('a')].every(n=>n.getAttribute('href')==='#bibi-attachment-'+a.id)).toBe(true);expect(node.querySelector('img')).toBeNull();expect(node.textContent).toContain('unknown');
});

it('keeps parsed answers across new API objects and invalidates changed result links',()=>{
 const parse=vi.spyOn(marked,'parse');
 try{
  const render=messageMarkdown(),text='[result](out.csv)';
  const a={id:'att_'+'a'.repeat(32),project_key:'p',name:'out.csv',kind:'text' as const,media_type:'text/plain',size:1,sha256:'fixture',source:'out.csv'};
  render(text,[]);for(let i=0;i<100;i++)render(text,[]);expect(parse).toHaveBeenCalledTimes(1);
  const linked=render(text,[a]);for(let i=0;i<100;i++)expect(render(text,[{...a}])).toBe(linked);expect(parse).toHaveBeenCalledTimes(2);
  expect(render(text,[{...a,id:'att_'+'b'.repeat(32)}])).not.toBe(linked);expect(parse).toHaveBeenCalledTimes(3);
  render(text+' changed',[a]);expect(parse).toHaveBeenCalledTimes(4);
 }finally{parse.mockRestore();}
});
