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


test('home is a bounded digest and community is an independent keyboard-accessible page',async({page})=>{
 const ids=Array.from({length:5},(_,i)=>`00000000-0000-4000-8000-${String(i+1).padStart(12,'0')}`),reads=[];
 const items=ids.map((id,i)=>({id,owner:2,name:['旅人','夏末','一颗星','阿宁','橙子'][i],kind:'post',parent:null,created_at:Math.floor(Date.now()/1000)-i*900,liked:false,likes:i}));
 await appFixture(page,{replies:{'/api/community':{account:1,enabled:true,items,next:null,server_time:1}}});
 await page.route(/\/api\/community\/[\da-f-]+\?/,route=>{const id=new URL(route.request().url()).pathname.split('/').at(-1);reads.push(id);return route.fulfill({json:{account:1,id,content:{title:`分享 ${ids.indexOf(id)+1}`,text:'一起分享游戏与生活。'.repeat(8),image:''}}});});
 await page.goto('/');const home=page.locator('.community-home');await expect(home.getByText('分享 3',{exact:true})).toBeVisible();await expect(home.locator('.community-latest-card')).toHaveCount(3);expect(reads).toHaveLength(3);await expect(home.locator('.feed-composer')).toHaveCount(0);await expect(home).not.toContainText('分享 4');
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);}
 await page.setViewportSize({width:1280,height:900});await home.locator('.community-latest').screenshot({path:test.info().outputPath('home-digest-desktop.png')});await page.setViewportSize({width:390,height:844});await home.locator('.community-latest').scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('home-digest-mobile.png')});
 await home.getByRole('button',{name:'进入社区 ↗',exact:true}).click();await expect(page).toHaveURL(/\/community$/);await expect(page.locator('.community-home')).toHaveCount(0);await expect(page.locator('.community-hub')).toBeVisible();await expect(page.getByText('分享 5',{exact:true})).toBeVisible();
 const composer=page.locator('.community-hub .community-publish');await expect(composer.getByLabel('动态正文')).toBeHidden();await composer.locator('summary').focus();await page.keyboard.press('Enter');await expect(composer.getByLabel('动态正文')).toBeVisible();await composer.locator('summary').press('Enter');
 await page.setViewportSize({width:1280,height:900});await page.locator('.community-hub').screenshot({path:test.info().outputPath('community-hub-desktop.png')});await page.setViewportSize({width:390,height:844});await page.getByRole('button',{name:'切换深浅主题',exact:true}).click();await page.locator('.community-hub').scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('community-hub-mobile-light.png'),animations:'disabled'});
 await page.goBack();await expect(page.locator('.app')).toHaveAttribute('data-view','home');await expect(home.locator('.feed-composer')).toHaveCount(0);
});

test('anonymous community routes expose the login entrance without member API requests',async({page})=>{
 const requests=await appFixture(page,{loggedIn:false});await page.goto('/community');await expect(page.getByRole('heading',{name:'登录后，加入这里的讨论'})).toBeVisible();expect(requests).not.toContain('/api/community');expect(requests).not.toContain('/api/friends');await page.getByRole('button',{name:'登录社区',exact:true}).click();await expect(page).toHaveURL(/\/login$/);
});

test('closed community has a clear state and foreign account digests never fetch body objects',async({page})=>{
 const requests=await appFixture(page,{replies:{'/api/community':{account:2,enabled:true,items:[{id:'00000000-0000-4000-8000-000000000001',name:'不应展示',owner:2,kind:'post',created_at:1,likes:0}],next:null,server_time:1}}});
 await page.goto('/');await expect(page.locator('.community-latest').getByRole('alert')).toContainText('账号已变化');expect(requests.filter(p=>p.startsWith('/api/community/'))).toHaveLength(0);await expect(page.locator('.community-home')).not.toContainText('不应展示');
 await page.route('**/api/community?**',route=>route.fulfill({json:{account:1,enabled:false,items:[],next:null,server_time:1}}));await page.getByRole('button',{name:'社区',exact:true}).click();await expect(page.locator('.community-hub')).toContainText('社区动态尚未开放');await expect(page.locator('.feed-composer')).toHaveCount(0);
});
