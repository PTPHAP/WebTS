import {test,expect} from '@playwright/test';
import {appFixture} from './app-fixture.mjs';

function event(id,changes={}){
 const now=Math.floor(Date.now()/1000);
 return {id,title:`活动 ${id}`,description:'合成活动内容',mine:false,starts:now+600,ends:now+3600,server:'fixture',channel:7,community:false,cancelled:false,attending:false,participants:1,...changes};
}

test('the inbox rejects a different accounts metadata instead of showing its private letter',async({page})=>{
 await appFixture(page,{replies:{'/api/notices':{account:2,unread:1,email_enabled:false,items:[{id:1,title:'其他账号的私密信件',created_at:1,read:false}]}}});
 await page.goto('/');await page.getByRole('button',{name:/站点信箱/}).click();
 await expect(page.locator('.inbox-dialog')).toContainText('账号已变化');
 await expect(page.locator('.inbox-dialog')).not.toContainText('其他账号的私密信件');
});

test('a late letter response cannot restore private content after authorization polling failed',async({page})=>{
 await page.clock.install();await appFixture(page);let revoked=false,finish;
 await page.route('**/api/notices',route=>route.fulfill({status:revoked?401:200,json:revoked?{error:'登录已失效'}:{account:1,unread:1,email_enabled:false,items:[{id:1,title:'合成信件',created_at:1,read:false}]}}));
 await page.route('**/api/notices/1',route=>{finish=()=>route.fulfill({json:{account:1,id:1,title:'迟到的信件',html:'<p>迟到的正文</p>',created_at:1}});});
 await page.goto('/');await page.getByRole('button',{name:/站点信箱/}).click();await page.getByRole('button',{name:/合成信件/}).click();await expect.poll(()=>typeof finish).toBe('function');
 revoked=true;await page.clock.fastForward(21000);await expect(page.locator('.inbox-dialog')).toContainText('登录已失效');await finish();
 await expect(page.locator('.letter-reading')).not.toContainText('迟到的正文');await expect(page.locator('.letter-row')).toHaveCount(0);
});

test('notification hydration fails closed before a saved quiet policy arrives',async({page})=>{
 await page.clock.install();await appFixture(page);let finish,mail=0;
 await page.route('**/api/preferences',route=>{finish=()=>route.fulfill({json:{account:1,initialized:true,preferences:{notifications:{quiet:true,messages:true,poke:true,friends:true,mail:true,activities:true,channel:true}}}});});
 await page.route('**/api/notices',route=>route.fulfill({json:{account:1,unread:mail,email_enabled:false,items:mail?[{id:mail,title:'合成信件',created_at:1,read:false}]:[]}}));
 await page.goto('/');await expect.poll(()=>typeof finish).toBe('function');mail=1;await page.clock.fastForward(10001);await page.evaluate(()=>window.dispatchEvent(new Event('focus')));await expect(page.getByRole('button',{name:'站点信箱，1封未读'})).toBeVisible();await expect(page.locator('.activity-alert')).toHaveCount(0);
 await finish();await expect(page.getByRole('button',{name:'通知设置 · 免打扰已开启'})).toBeVisible();mail=2;await page.clock.fastForward(11000);await expect(page.getByRole('button',{name:'站点信箱，2封未读'})).toBeVisible();await expect(page.locator('.activity-alert')).toHaveCount(0);
});

test('notification preferences sync and quiet mode retains unread mail without interrupting the page',async({page})=>{
 await page.clock.install();await appFixture(page);let policy={quiet:false,messages:true,poke:true,friends:true,mail:true,activities:true,channel:true};let mail=0,saved=[];
 await page.route('**/api/preferences',route=>{const body=route.request().postDataJSON();if(body){saved.push(body);policy={...policy,...body.patch.notifications};}return route.fulfill({json:{account:1,initialized:true,preferences:{notifications:policy}}});});
 await page.route('**/api/notices',route=>route.fulfill({json:{account:1,unread:mail,email_enabled:false,items:mail?[{id:mail,title:'合成更新通知',created_at:1,read:false}]:[]}}));
 await page.goto('/');await page.getByRole('button',{name:'通知设置',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'通知与免打扰'});
 await expect(dialog.getByText('正在同步通知偏好')).toHaveCount(0);
 await dialog.getByLabel('免打扰 · 暂停所有非必要提醒').check();await page.clock.fastForward(400);
 await expect.poll(()=>saved.at(-1)?.patch.notifications.quiet).toBe(true);await expect(dialog.getByLabel('聊天、私信与社区回复')).toBeDisabled();
 await dialog.getByRole('button',{name:'关闭',exact:true}).click();mail=1;await page.clock.fastForward(21000);
 await expect(page.getByRole('button',{name:'站点信箱，1封未读'})).toBeVisible();await expect(page.locator('.activity-alert')).toHaveCount(0);
 await page.getByRole('button',{name:'通知设置 · 免打扰已开启'}).click();await dialog.getByLabel('免打扰 · 暂停所有非必要提醒').uncheck();await page.clock.fastForward(400);await dialog.getByRole('button',{name:'关闭',exact:true}).click();
 mail=2;await page.clock.fastForward(21000);await expect(page.locator('.activity-alert')).toContainText('收到新的站点信件');
 await page.clock.fastForward(7100);await expect(page.locator('.activity-alert')).toHaveCount(0);
 await page.reload();await page.getByRole('button',{name:'通知设置',exact:true}).click();await expect(dialog.getByLabel('免打扰 · 暂停所有非必要提醒')).not.toBeChecked();
 if(test.info().project.name.includes('mobile')){for(const width of [320,390]){await page.setViewportSize({width,height:844});expect(await dialog.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);}}
 await page.screenshot({path:test.info().outputPath('notification-settings.png'),fullPage:true,animations:'disabled'});
});
test('friend polling rejects an account switch between the own-account and friend-list requests',async({page})=>{
 await appFixture(page,{replies:{'/api/friends':{account:2,friends:[{id:3,name:'其他账号的好友',status:1,requester:2,blocked_by:0}],unread:0,latest_message:0}}});
 await page.goto('/app');
 if(test.info().project.name.includes('mobile'))await page.getByRole('navigation',{name:'手机主导航'}).getByRole('button',{name:'好友',exact:true}).click();
 else await page.getByRole('button',{name:'WebTS 好友',exact:true}).click();
 const panel=page.locator('.friends-panel:visible,.friends-mobile-page:visible');
 await expect(panel).toContainText('账号已变化');
 await expect(panel).not.toContainText('其他账号的好友');
});
