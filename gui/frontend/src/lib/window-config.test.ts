import {expect,it} from 'vitest';
import {readFileSync} from 'node:fs';
const json=(file:string)=>JSON.parse(readFileSync(new URL('../../../'+file,import.meta.url),'utf8'));
it('Windows removes native chrome while retaining the shared window geometry and required controls',()=>{
 const base=json('tauri.conf.json').app.windows[0],windows=json('tauri.windows.conf.json').app.windows[0];
 for(const field of ['label','title','width','height','minWidth','minHeight','resizable','dragDropEnabled'])expect(windows[field]).toEqual(base[field]);
 expect(windows.decorations).toBe(false);expect(windows.shadow).toBe(true);
 expect(base.decorations).not.toBe(false);expect(base.titleBarStyle).toBe('Overlay');
 const permissions=json('capabilities/default.json').permissions;
 for(const operation of ['start-dragging','minimize','toggle-maximize','close'])expect(permissions).toContain('core:window:allow-'+operation);
});
