import {test,expect} from '@playwright/test';
import {appFixture} from './app-fixture.mjs';
const row={id:'a'.repeat(64),kind:'account',target:'2',reason:'<img src=x onerror="window.stolen=true">\n仅作为文字',reply:'',note:'内部备注',owner:1,status:'pending',revision:0,created_at:1,updated_at:1,audit:[]};
async function open(page){await page.getByRole('button',{name:'账号与个人资料'}).click();await page.getByRole('button',{name:'举报与处理进度',exact:true}).click();return page.getByRole('dialog',{name:'举报与处理进度'});}
test('report submissions are scoped, text-only and usable on desktop and phone',async({page})=>{
 await appFixture(page,{replies:{'/api/friends':{account:1,friends:[{id:2,name:'合成好友',status:1,requester:1,blocked_by:0,public_key:'',device:''}],unread:0}}});let submitted,items=[];
 await page.route('**/api/reports**',route=>{if(route.request().method()==='POST'){submitted=route.request().postDataJSON();items=[row];return route.fulfill({json:{account:1,id:row.id}});}return route.fulfill({json:{account:1,items,next:null}});});
 await page.goto('/app');const dialog=await open(page);await dialog.getByLabel('举报对象').selectOption('account:2');await dialog.getByLabel('原因与必要说明').fill('必要说明，不包含聊天密钥');await dialog.getByRole('button',{name:'提交举报'}).click();
 await expect.poll(()=>submitted?.account).toBe(1);expect(submitted.target).toBe('2');expect(Object.keys(submitted).sort()).toEqual(['account','kind','reason','request_id','target']);
 await expect(dialog).toContainText('举报已提交');await dialog.locator('.report-case>summary').click();await expect(dialog.locator('.report-evidence')).toContainText('<img src=x');await expect(dialog.locator('.report-evidence img')).toHaveCount(0);expect(await page.evaluate(()=>window.stolen)).toBeUndefined();
 expect(await dialog.locator('.report-center').evaluate(n=>n.scrollWidth<=n.clientWidth+1)).toBe(true);
 await dialog.screenshot({path:test.info().outputPath('reports.png')});
});
test('a different accounts report response is rejected without showing evidence',async({page})=>{
 await appFixture(page);await page.route('**/api/reports**',route=>route.fulfill({json:{account:2,items:[{...row,reason:'other-private-evidence'}],next:null}}));await page.goto('/');const dialog=await open(page);
 await expect(dialog).toContainText('账号已变化');await expect(dialog).not.toContainText('other-private-evidence');
});
test('closing the report dialog invalidates a late response',async({page})=>{
 await appFixture(page);let finish;await page.route('**/api/reports**',route=>{finish=()=>route.fulfill({json:{account:1,items:[{...row,reason:'late-evidence'}],next:null}});});await page.goto('/');const dialog=await open(page);await expect.poll(()=>typeof finish).toBe('function');await dialog.getByRole('button',{name:'关闭',exact:true}).click();await finish().catch(()=>{});await expect(page.locator('.report-center')).toHaveCount(0);await expect(page.locator('body')).not.toContainText('late-evidence');
});
test('report refresh failure clears old evidence and permits an explicit retry',async({page})=>{
 await appFixture(page);let denied=false;await page.route('**/api/reports**',route=>route.fulfill(denied?{status:401,json:{error:'登录已失效'}}:{json:{account:1,items:[row],next:null}}));await page.goto('/');const dialog=await open(page);await expect(dialog.locator('.report-case')).toHaveCount(1);denied=true;await dialog.getByRole('button',{name:'刷新',exact:true}).click();await expect(dialog).toContainText('登录已失效');await expect(dialog.locator('.report-case')).toHaveCount(0);denied=false;await dialog.getByRole('button',{name:'重新读取',exact:true}).click();await expect(dialog.locator('.report-case')).toHaveCount(1);
});
test('admin decisions carry the displayed revision and do not expose HTML execution',async({page})=>{
 await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'fixture@example.invalid',is_admin:true}}}});let decision;
 await page.route('**/api/admin/reports**',route=>{if(route.request().method()==='POST'){decision=route.request().postDataJSON();return route.fulfill({json:{account:1,message:'已保存'}});}return route.fulfill({json:{account:1,items:[row],next:null}});});
 await page.goto('/');await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'举报处理'}).click();const center=page.locator('.report-center');await center.locator('.report-case>summary').click();await center.getByLabel('处理状态').selectOption('resolved');await center.getByLabel('给举报人的处理说明').fill('处理完成');await center.getByLabel('内部备注 · 仅管理员可见').fill('后台必要备注');await center.getByRole('button',{name:'保存处理结果'}).click();await expect.poll(()=>decision?.revision).toBe(0);expect(decision.account).toBe(1);expect(decision.status).toBe('resolved');await expect(center).toContainText('处理已保存');
});
