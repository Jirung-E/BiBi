import {expect,type Page} from '@playwright/test';

// Check the rendered, overflowing owners rather than a list of class names:
// this catches new panels/menus that forget the overlay scrollbar action.
export async function expectOverlayScrolling(page:Page){
 const native=await page.locator('body *').evaluateAll(elements=>elements.filter(e=>{
  const style=getComputedStyle(e);
  return e.getClientRects().length&&!e.closest('[inert]')&&e.clientWidth>0&&e.clientHeight>0&&(
   /auto|scroll/.test(style.overflowY)&&e.scrollHeight>e.clientHeight+1||
   /auto|scroll/.test(style.overflowX)&&e.scrollWidth>e.clientWidth+1
  )&&style.scrollbarWidth!=='none';
 }).map(e=>e.tagName+'.'+e.className+': '+(e.getAttribute('aria-label')??e.id)));
 expect(native,'all overflowing panels must retain overlay scrollbars').toEqual([]);
}
