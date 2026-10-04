// @vitest-environment jsdom
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
const native=vi.hoisted(()=>({setTheme:vi.fn()}));
vi.mock('./api',()=>({isDesktop:()=>true}));
vi.mock('@tauri-apps/api/window',()=>({getCurrentWindow:()=>native}));
let stop=()=>{};
beforeEach(()=>{
 vi.resetModules();native.setTheme.mockReset();localStorage.clear();
 const media=Object.assign(new EventTarget(),{matches:false});
 vi.stubGlobal('matchMedia',()=>media);
});
afterEach(()=>{stop();vi.unstubAllGlobals();vi.restoreAllMocks();});
it('serializes native light/dark/system changes and recovers after a native failure',async()=>{
 const warning=vi.spyOn(console,'warn').mockImplementation(()=>{});
 native.setTheme.mockRejectedValueOnce(new Error('Window not ready'));
 const module=await import('./theme');stop=module.initTheme();
 module.setTheme('dark');module.setTheme('light');module.setTheme('system');
 await vi.waitFor(()=>expect(native.setTheme.mock.calls.map(args=>args[0])).toEqual([null,'dark','light',null]));
 expect(document.documentElement.dataset.theme).toBe('light');
 expect(localStorage.getItem('bibi:theme')).toBe('system');expect(warning).toHaveBeenCalledOnce();
});
