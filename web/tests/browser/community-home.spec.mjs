import {test,expect} from '@playwright/test';
import {appFixture} from './app-fixture.mjs';

test('public homepage does not request private community data',async({page})=>{
  const requests=await appFixture(page,{loggedIn:false});
  await page.goto('/');
  await expect(page.locator('.site-home')).toBeVisible();
  await expect(page.locator('.community-home')).toHaveCount(0);
  expect(requests).not.toContain('/api/events');
  expect(requests).not.toContain('/api/friends');
});

test('community homepage shows authorized data, escapes content and keeps mobile cards within the viewport',async({page})=>{
  const requests=await appFixture(page,{replies:{
    '/api/events':{account:1,server_time:1,items:[{id:'event',title:'今晚一起看比赛',description:'',starts:301,ends:3901,server:'fixture',channel:12,community:true,cancelled:false,attending:true,participants:3}]},
    '/api/friends':{account:1,friends:[{id:2,name:'<img src=x onerror=alert(1)>',status:1,blocked_by:0,online:true},{id:3,name:'待接受的好友',status:0,blocked_by:0},{id:4,name:'已屏蔽的好友',status:2,blocked_by:1},{id:5,name:'状态隐藏的朋友',status:1,blocked_by:0}]},
    '/api/preferences':{account:1,initialized:true,preferences:{favorites:[{server:'fixture',uid:'uid',channel:12,name:'一起聊天的频道'}]}},
  }});
  await page.goto('/');
  const home=page.locator('.community-home');
  await expect(home).toBeVisible();
  expect(requests).not.toContain('/api/events');
  await expect(home).not.toContainText('今晚一起看比赛');
  await expect(page.getByRole('button',{name:'社区活动',exact:true})).toHaveCount(0);
  await expect(home).toContainText('2 位好友 · 1 位分享了在线状态');
  await expect(home).toContainText('<img src=x onerror=alert(1)>');
  await expect(home).not.toContainText('待接受的好友');
  await expect(home).not.toContainText('已屏蔽的好友');
  await expect(home).not.toContainText('navigation@example.invalid');
  await expect(home.getByText('状态已隐藏',{exact:true})).toBeVisible();
  await expect(home.locator('img[src="x"]')).toHaveCount(0);
  await expect(home.getByRole('button',{name:/一起聊天的频道/})).toBeVisible();
  if(test.info().project.name.includes('mobile')){
    for(const width of [320,390]){
      await page.setViewportSize({width,height:844});
      expect(await home.evaluate(node=>[...node.querySelectorAll('.community-favorites>button,.community-people>button')].every(n=>n.getBoundingClientRect().width<=n.parentElement.clientWidth+1))).toBe(true);
      expect(await home.evaluate(node=>node.scrollWidth-node.clientWidth)).toBeLessThanOrEqual(1);
      expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
    }
  }
});

test('community homepage refuses replies for a different logged-in account',async({page})=>{
  await appFixture(page,{replies:{
    '/api/events':{account:2,server_time:1,items:[{title:'其他账号的私有活动'}]},
    '/api/friends':{account:2,friends:[{id:3,name:'其他账号的好友',status:1,blocked_by:0}]},
  }});
  await page.goto('/');
  const home=page.locator('.community-home');

  await expect(home).toContainText('暂时无法读取好友');
  await expect(home).not.toContainText('其他账号的私有活动');
  await expect(home).not.toContainText('其他账号的好友');
});
