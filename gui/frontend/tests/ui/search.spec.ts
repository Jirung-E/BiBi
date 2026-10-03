import {test,expect} from '@playwright/test';
for(const [width,scale] of [[1280,1],[390,1],[320,2]])test(`session search opens a saved conversation and supports back at ${width}px / ${scale}`,async({page},testInfo)=>{
 if(testInfo.project.metadata.platform)await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{get:()=>'Win32'}));
 await page.addInitScript(uiScale=>localStorage.setItem('bibi:appearance',JSON.stringify({uiScale,halfLife:30,floor:.15})),scale);
 await page.setViewportSize({width,height:844});
 await page.goto('/?view=canvas&project=layout-project');
 await expect(page.locator('.board')).toBeVisible();
 if(!await page.getByRole('button',{name:'세션 검색',exact:true}).isVisible())await page.getByRole('button',{name:'사이드바 열기',exact:true}).click();
 await page.getByRole('button',{name:'세션 검색',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'세션 검색',exact:true});await expect(dialog).toBeVisible();
 const search=dialog.getByRole('searchbox',{name:'세션 검색어'});await expect(search).toBeFocused();await search.fill('no-such-session-9351');
 await expect(dialog.getByText('일치하는 세션이 없습니다.')).toBeVisible();
 await search.fill('');const result=dialog.locator('.search-result').first();await expect(result).toBeEnabled();
 const bounds=await dialog.boundingBox();expect(bounds!.x).toBeGreaterThanOrEqual(0);expect(bounds!.x+bounds!.width).toBeLessThanOrEqual(width+1);
 await page.screenshot({path:testInfo.outputPath('session-search.png')});await search.press('Enter');await expect(dialog).toHaveCount(0);await expect(page).toHaveURL(/view=conversation/);
 await page.goBack();await expect(dialog).toBeVisible();await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);
});
test('search failure is visible and a later query recovers',async({page},testInfo)=>{
 if(testInfo.project.metadata.platform)await page.addInitScript(()=>Object.defineProperty(navigator,'platform',{get:()=>'Win32'}));
 let fail=true;await page.route('**/api/sessions/search?*',async route=>{if(fail)return route.fulfill({status:503,json:{error:'검색 연결 실패'}});await route.continue();});
 await page.goto('/?project=layout-project');await page.getByRole('button',{name:'세션 검색',exact:true}).click();
 await expect(page.getByRole('alert')).toHaveText('검색 연결 실패');fail=false;await page.getByRole('searchbox',{name:'세션 검색어'}).fill('layout');
 await expect(page.locator('.search-result').first()).toBeVisible();
});
