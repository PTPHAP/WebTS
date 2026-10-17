import {test,expect} from '@playwright/test';
import {appFixture} from './app-fixture.mjs';

test('administrator adds and switches storage without exposing credentials or losing mobile controls',async({page})=>{
 const writes=[],secret='synthetic-secret-only';
 const config={enabled:true,endpoint:'https://cn-sy1.rains3.com',bucket:'fixture-old',region:'us-east-1',retention_days:7,credentials_set:true};
 let active='default',profiles=[{id:'default',name:'原有存储',objects:3,storage:config}];
 await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'storage-admin@example.invalid',is_admin:true}}}});
 await page.route('**/api/admin/storage/profiles',async route=>{
  if(route.request().method()==='POST'){
   const b=route.request().postDataJSON();writes.push(b);
   if(b.password!=='synthetic-password')return route.fulfill({status:401,json:{error:'当前密码不正确'}});
   if(b.action==='activate')active=b.id;
   else if(b.action==='save'){b.id||='a'.repeat(64);profiles=[...profiles.filter(p=>p.id!==b.id),{id:b.id,name:b.name,objects:0,storage:{...b.storage,access_key:undefined,secret_key:undefined,credentials_set:true}}];}
   return route.fulfill({json:{id:b.id,message:'已保存'}});
  }
  return route.fulfill({json:{active,profiles}});
 });
 await page.goto('/');await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'加密临时存储'}).click();
 const select=page.getByLabel('选择存储配置');await select.selectOption('new');
 await page.getByLabel('配置名称').fill('社区图片仓库');await page.getByLabel('允许此配置接收新对象').check();
 await page.getByLabel('存储服务').selectOption('rainyun');await page.getByLabel('私有桶名称').fill('fixture-new');
 await page.getByLabel('Access Key',{exact:true}).fill('synthetic-access');await page.getByLabel('Secret Key',{exact:true}).fill(secret);
 await page.getByLabel('确认管理员密码').fill('synthetic-password');await page.getByRole('button',{name:'保存并热加载',exact:true}).click();
 await expect(select).toHaveValue('a'.repeat(64));await expect(page.getByLabel('Secret Key',{exact:true})).toHaveValue('');await expect(page.getByLabel('确认管理员密码')).toHaveValue('');
 await page.getByLabel('确认管理员密码').fill('synthetic-password');await page.getByRole('button',{name:'设为默认写入'}).click();
 await expect(select).toHaveValue('a'.repeat(64));await expect(page.locator('.storage-manager')).toContainText('当前默认：社区图片仓库');
 expect(writes.map(v=>v.action)).toEqual(['save','activate']);expect(writes[0].storage.secret_key).toBe(secret);
 expect(await page.locator('.storage-manager').textContent()).not.toContain(secret);
 await select.selectOption('default');await expect(page.locator('.storage-manager')).toContainText('旧对象始终从原桶读取和清理');await expect(page.getByRole('button',{name:'删除配置',exact:true})).toHaveCount(0);
 await page.setViewportSize({width:390,height:844});await page.getByLabel('选择存储配置').scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('storage-profiles-mobile.png')});
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await page.locator('.storage-manager').evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);}
});
