import {expect,type Page} from '@playwright/test';

export async function showSidebar(page:Page){
 if(!await page.locator('.app-shell').evaluate(e=>e.classList.contains('sidebar-expanded'))&&!await page.getByRole('dialog',{name:'사이드바',exact:true}).isVisible()){
  await page.getByRole('button',{name:'사이드바 열기',exact:true}).click();
 }
 await expect(page.getByRole('navigation',{name:'주요 메뉴'})).toBeVisible();
}
export async function sidebarAction(page:Page,name:string){
 await showSidebar(page);
 await page.getByRole('button',{name,exact:true}).click();
}
