import {it,expect} from 'vitest';
import {diagramProblem,prepareDiagram} from './diagram-source';
it('keeps SVG line breaks and the plain title while removing source initialization',()=>{
 const body='flowchart LR\n A["첫 줄<br/>둘째 줄"] --> B[완료]';
 const raw='---\ntitle: 업무 흐름\nconfig:\n  securityLevel: loose\n  themeCSS: "@import url(https://invalid.example/a.css)"\n---\n%%{init: {"theme":"dark","securityLevel":"loose"}}%%\n'+body;
 const ready=prepareDiagram(raw);expect(ready.problem).toBeNull();expect(ready.configured).toBe(true);
 expect(ready.code).toContain('title: "업무 흐름"');expect(ready.code).toContain(body);
 expect(ready.code).not.toMatch(/securityLevel|themeCSS|https:|%%\{/);expect(raw).toContain('securityLevel');
 expect(diagramProblem('flowchart LR\n A["<b>텍스트</b><br>둘째 줄"]')).toBeNull();
});
it('rejects resource loading and active HTML with a specific reason',()=>{
 for(const source of ['flowchart LR\n A@{img:"https://invalid.example/x"}','flowchart LR\n A["<img src=x>"]',
  'flowchart LR\n A["<br onmouseover=alert(1)>"]','flowchart LR\n A-->B\n classDef default fill:u\\72l(https://invalid.example/x)',
  'flowchart LR\n A-->B\n classDef default fill:u/**/rl(https://invalid.example/x)'])expect(diagramProblem(source)).not.toBeNull();
});
it('reports partial headers and directives without evaluating aliases',()=>{
 expect(diagramProblem('---\ntitle: 아직 작성 중')).toContain('머리말');
 expect(diagramProblem('%%{init: {')).toContain('지시문');
 expect(diagramProblem('---\na: &a [1]\nb: *a\n---\nflowchart LR\n A-->B')).toContain('YAML');
 expect(diagramProblem('flowchart LR\n A-->B')).toBeNull();
});
