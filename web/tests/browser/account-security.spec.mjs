import {test,expect} from '@playwright/test';
import {appFixture} from './app-fixture.mjs';

const session=(id,current=false)=>({id:id.repeat(64),current,browser:'Edge',platform:'Windows',created_at:1780000000,expires:1880000000,remembered:current?false:true});
async function fixture(page){
  const requests=await appFixture(page);
  let items=[session('a',true),session('b')];const writes=[];
  await page.route('**/api/auth/sessions**',async route=>{
    const request=route.request();
    requests.push(new URL(request.url()).pathname);
    if(request.method()==='POST'){
      const data=request.postDataJSON();writes.push(data);
      if(data.password!=='synthetic-password')return route.fulfill({status:401,json:{error:'当前密码不正确'}});
      items=items.filter(s=>s.current);return route.fulfill({json:{account:1,removed:1}});
    }
    return route.fulfill({json:{account:1,items,total:items.length}});
  });
  return {requests,writes};
}

test('security center is loaded on demand and revokes other sessions only after confirmation',async({page})=>{
  const{requests,writes}=await fixture(page);await page.goto('/');
  const home=page.locator('.community-home');await expect(home).toBeVisible();
  expect(requests.some(p=>p.includes('/auth/sessions'))).toBe(false);
  await home.getByRole('button',{name:'账号安全',exact:true}).click();
  const panel=page.getByRole('dialog',{name:'账号安全中心'});
  await expect(panel.getByText('2 个有效登录会话',{exact:true})).toBeVisible();
  expect(requests.filter(p=>p==='/api/auth/sessions')).toHaveLength(1);
  await expect(panel.locator('.security-current')).toHaveText('当前登录');
  await expect(panel.getByRole('button',{name:'退出此会话'})).toHaveCount(1);
  await panel.getByRole('button',{name:'退出此会话'}).click();
  await expect(panel.getByRole('button',{name:'确认退出',exact:true})).toBeDisabled();
  await panel.getByLabel('验证当前网站密码').fill('wrong');await panel.getByRole('button',{name:'确认退出',exact:true}).click();
  await expect(panel.getByRole('alert')).toContainText('当前密码不正确');
  await expect(panel.getByLabel('验证当前网站密码')).toHaveCount(0);
  expect(writes).toEqual([{account:1,target:'b'.repeat(64),password:'wrong'}]);
  await panel.getByRole('button',{name:'刷新列表'}).click();await panel.getByRole('button',{name:'退出此会话'}).click();
  await panel.getByLabel('验证当前网站密码').fill('synthetic-password');await panel.getByRole('button',{name:'确认退出',exact:true}).click();
  await expect(panel.getByText('1 个有效登录会话',{exact:true})).toBeVisible();
  await expect(panel.getByRole('status')).toContainText('已退出1个登录会话');
  await expect(panel.locator('.security-confirm')).toHaveCount(0);
  await expect(panel.getByRole('button',{name:'保留当前登录，退出其他会话'})).toBeDisabled();
  await expect(panel.locator('.security-success')).toHaveCount(0,{timeout:10000});
});

test('account mismatch and failed reads never retain session rows',async({page})=>{
  await fixture(page);await page.goto('/');await page.locator('.community-home').getByRole('button',{name:'账号安全',exact:true}).click();
  const panel=page.getByRole('dialog',{name:'账号安全中心'});await expect(panel.locator('.security-session')).toHaveCount(2);
  await panel.getByRole('button',{name:'退出此会话'}).click();
  await panel.getByLabel('验证当前网站密码').fill('must-be-cleared');
  await page.route('**/api/auth/sessions?**',route=>route.fulfill({json:{account:2,total:1,items:[{...session('c'),browser:'其他账号的设备'}]}}));
  await panel.getByRole('button',{name:'刷新列表'}).click();
  await expect(panel.getByRole('alert')).toContainText('账号已变化');await expect(panel.locator('.security-session')).toHaveCount(0);await expect(panel).not.toContainText('其他账号的设备');
  await expect(panel.locator('.security-confirm')).toHaveCount(0);
  await expect(panel.locator('input[type=password]')).toHaveCount(0);
  await page.route('**/api/auth/sessions?**',route=>route.fulfill({status:503,json:{error:'繁忙，请稍后重试'}}));
  await panel.getByRole('button',{name:'刷新列表'}).click();await expect(panel.getByRole('alert')).toContainText('繁忙');await expect(panel.locator('.security-session')).toHaveCount(0);
});

test('session cards and confirmation fit narrow screens and reopening starts a fresh read',async({page})=>{
  await fixture(page);await page.goto('/');await page.getByRole('button',{name:'账号与个人资料'}).click();
  await page.getByRole('button',{name:'账号安全 · 登录设备与会话'}).click();
  let panel=page.getByRole('dialog',{name:'账号安全中心'});await expect(panel.locator('.security-session')).toHaveCount(2);
  for(const width of [320,390,1280]){
    await page.setViewportSize({width,height:844});
    expect(await panel.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);
    expect(await panel.locator('.account-security').evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);
  }
  await page.setViewportSize({width:390,height:844});await panel.getByRole('button',{name:'保留当前登录，退出其他会话'}).click();
  await panel.getByLabel('验证当前网站密码').fill('not-saved');
  await panel.screenshot({path:test.info().outputPath('account-security-mobile.png')});
  await panel.getByRole('button',{name:'关闭',exact:true}).click();
  await page.locator('.community-home').getByRole('button',{name:'账号安全',exact:true}).click();
  panel=page.getByRole('dialog',{name:'账号安全中心'});await expect(panel.locator('.security-session')).toHaveCount(2);await expect(panel.locator('input[type=password]')).toHaveCount(0);
  await panel.getByRole('button',{name:'保留当前登录，退出其他会话'}).click();await panel.getByLabel('验证当前网站密码').fill('synthetic-password');await panel.getByRole('button',{name:'确认退出',exact:true}).click();
  await expect(panel.getByText('1 个有效登录会话',{exact:true})).toBeVisible();
});

test('closing a pending read cannot fill the next security dialog with a late reply',async({page})=>{
  await fixture(page);let first,reads=0;
  await page.route('**/api/auth/sessions?**',async route=>{
    reads++;
    if(reads===1){first=route;return;}
    await route.fulfill({json:{account:1,total:1,items:[session('a',true)]}});
  });
  await page.goto('/');await page.locator('.community-home').getByRole('button',{name:'账号安全',exact:true}).click();
  await expect.poll(()=>reads).toBe(1);
  await page.getByRole('dialog',{name:'账号安全中心'}).getByRole('button',{name:'关闭',exact:true}).click();
  await page.locator('.community-home').getByRole('button',{name:'账号安全',exact:true}).click();
  const panel=page.getByRole('dialog',{name:'账号安全中心'});await expect(panel.getByText('1 个有效登录会话',{exact:true})).toBeVisible();
  await first.fulfill({json:{account:1,total:2,items:[{...session('b'),browser:'旧请求设备不得出现'}]}}).catch(()=>{});
  await expect(panel.locator('.security-session')).toHaveCount(1);await expect(panel).not.toContainText('旧请求设备不得出现');
});
