import {test,expect} from '@playwright/test';
import {readFile} from 'node:fs/promises';
import {appFixture} from './app-fixture.mjs';

test('diagnostic panel and download identify the loaded page despite a newer server build',async({page})=>{
 const requests=await appFixture(page,{replies:{'/api/client-build':{bundle:'/assets/index-newer-on-server.js'}}});
 const errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto('/app');
 const current=await page.locator('script[type="module"][src]').evaluateAll(nodes=>nodes.map(n=>new URL(n.src).pathname).find(path=>/^\/assets\/index-[\w-]+\.js$/.test(path)));
 expect(current).toBeTruthy();await expect(page.locator('.update-notice')).toContainText('新版界面已就绪');
 await page.getByRole('button',{name:'语音设备设置',exact:true}).click();
 const panel=page.locator('.voice-diagnostics');await expect(panel).toContainText(current.slice(8));
 await expect(panel).not.toContainText('newer-on-server');await expect(panel).toContainText('未启动');await expect(panel).toContainText('0 / 0 / 0');
 const [download]=await Promise.all([page.waitForEvent('download'),panel.getByRole('button',{name:'下载脱敏诊断报告'}).click()]);
 const report=JSON.parse(await readFile(await download.path(),'utf8'));expect(report.format).toBe('webts-diagnostic-v5');expect(report.frontendBundle).toBe(current);
 expect(report.runtime).toEqual({context:'inactive',processing:'inactive',receivers:0,players:0,pausedPlayers:0});
 expect(JSON.stringify(report)).not.toMatch(/navigation@example|页面切换验收|fixture|127\.0\.0\.1|newer-on-server/);
 for(const width of [320,390,1280]){
  await page.setViewportSize({width,height:844});
  expect(await panel.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
 }
 await page.setViewportSize({width:390,height:844});await panel.evaluate(n=>{const dialog=n.closest('dialog');dialog.scrollTop+=n.getBoundingClientRect().top-dialog.getBoundingClientRect().top-90;});await page.screenshot({path:test.info().outputPath('voice-diagnostics-mobile.png')});
 expect(errors).toEqual([]);expect(requests.filter(path=>path==='/api/client-build')).toHaveLength(1);
});
