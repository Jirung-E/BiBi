// @vitest-environment jsdom
import {it,expect} from 'vitest';
import {markdown} from './markdown';
it('renders headings, code, lists and GFM tables while removing active HTML and URLs',()=>{
 const html=markdown('# 제목\n\n**굵게**\n\n- 하나\n- 둘\n\n```rust\nfn main() {}\n```\n\n| 열 | 값 |\n|---|---|\n| a | b |\n\n<script>alert(1)</script><img src=x onerror=alert(1)>\n\n[위험](javascript:alert(1)) <a href="https://example.com" onclick="alert(1)">안전한 링크</a>');
 const node=document.createElement('div');node.innerHTML=html;
 expect(node.querySelector('h1')?.textContent).toBe('제목');expect(node.querySelectorAll('li')).toHaveLength(2);expect(node.querySelector('pre code')?.textContent).toContain('fn main');expect(node.querySelector('table')).not.toBeNull();
 expect(node.querySelector('script,img,[onclick],[onerror]')).toBeNull();expect([...node.querySelectorAll('a[href]')].some(a=>a.getAttribute('href')?.startsWith('javascript:'))).toBe(false);expect(node.querySelector('a[href]')?.getAttribute('href')).toBe('https://example.com');
});
