import {invoke} from '@tauri-apps/api/core';

/** Keep real macOS controls on the same center line as the HTML toolbar. */
export function windowChrome(toolbar:HTMLElement,enabled:boolean) {
 if(!enabled)return;
 const shell=toolbar.closest<HTMLElement>('.app-shell')!;
 const anchor=shell.querySelector<HTMLElement>('.native-controls-anchor')!;
 let frame=0,active=true,last='';
 function sync(){
  frame=0;
  const box=toolbar.getBoundingClientRect();
  shell.style.setProperty('--mac-toolbar-height',box.height+'px');
  const left=parseFloat(getComputedStyle(anchor).paddingLeft),centerY=box.y+box.height/2;
  const key=left+':'+centerY;
  if(last===key)return;
  last=key;
  void invoke('set_window_chrome',{left,centerY}).catch(error=>{
   if(active){last='';console.error('Window chrome:',error);}
  });
 }
 function schedule(){if(!frame)frame=requestAnimationFrame(sync);}
 function restore(){last='';schedule();}
 const observer=new ResizeObserver(schedule);
 observer.observe(toolbar);observer.observe(anchor);
 window.addEventListener('focus',restore);
 schedule();
 return {destroy(){active=false;cancelAnimationFrame(frame);observer.disconnect();window.removeEventListener('focus',restore);}};
}
