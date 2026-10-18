import {test,expect} from '@playwright/test';
import {appFixture} from './app-fixture.mjs';
const home={site_name:'WebTS',site_icon:'',footer_html:'',operator:'',contact:'',data_details:'',privacy_policy:'',terms:'',title:'合成首页',subtitle:'隔离图片检查',image:'',announcements:[{id:'crop',title:'裁剪公告',text:'展示与裁剪一致',image:'',enabled:true}]};
async function picture(page){return page.evaluate(()=>{const c=document.createElement('canvas');c.width=300;c.height=600;const x=c.getContext('2d');for(const [y,color] of [[0,'#ff0000'],[200,'#00ff00'],[400,'#0000ff']]){x.fillStyle=color;x.fillRect(0,y,300,200);}return c.toDataURL('image/png');});}
test('public and community announcement frames preserve the full cropped image',async({page})=>{
 const image=await picture(page);
 for(const loggedIn of [false,true]){
  await appFixture(page,{loggedIn,replies:{'/api/site':{...home,announcements:[{...home.announcements[0],image}]}}});
  await page.goto('/');const img=page.locator(loggedIn?'.community-announcements article>img':'.announcement-card>img');
  await expect(img).toBeVisible();await img.scrollIntoViewIfNeeded();await expect.poll(()=>img.evaluate(n=>n.complete&&n.naturalHeight===600)).toBe(true);
  const shape=await img.evaluate(n=>{const s=getComputedStyle(n);return {ratio:n.clientWidth/n.clientHeight,fit:s.objectFit};});
  expect(shape.fit).toBe('contain');expect(shape.ratio).toBeCloseTo(16/9,1);
 }
});
test('announcement cropping previews the display frame and saves exactly the selected region',async({page})=>{
 const image=await picture(page);let saved;
 await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'admin@example.invalid',is_admin:true}},'/api/admin/home':home}});
 await page.route('**/api/admin/home',route=>{if(route.request().method()==='POST'){saved=route.request().postDataJSON().home;return route.fulfill({json:saved});}return route.fulfill({json:home});});
 await page.goto('/');await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'外观、协议与公告'}).click();
 const card=page.locator('.admin-home .identity-card').filter({has:page.getByLabel('公告标题')});
 await card.locator('input[type=file]').setInputFiles({name:'portrait.png',mimeType:'image/png',buffer:Buffer.from(image.split(',')[1],'base64')});
 const crop=page.locator('.image-cropper');await expect(crop.getByLabel('裁剪比例')).toHaveValue('free');
 await expect(crop.getByLabel('裁剪结果实时预览')).toBeVisible();
 const original=await crop.getByLabel('裁剪结果实时预览').evaluate(n=>n.width/n.height);expect(original).toBeCloseTo(.5,2);
 await crop.getByLabel('裁剪比例').selectOption('free');
 await crop.getByLabel('右下裁剪角').press('Shift+ArrowUp');
 await crop.getByLabel('移动裁剪选区').press('Shift+ArrowDown');
 await crop.screenshot({path:test.info().outputPath('announcement-crop.png')});
 const preview=await crop.getByLabel('裁剪结果实时预览').evaluate(n=>({width:n.width,height:n.height}));
 await crop.getByRole('button',{name:'使用裁剪图片'}).click();
 const img=card.getByAltText('公告展示预览');await expect(img).toBeVisible();
 await card.screenshot({path:test.info().outputPath('announcement-preview.png')});
 const src=await img.getAttribute('src');
 await expect.poll(()=>img.evaluate(n=>n.complete&&n.naturalWidth>0)).toBe(true);
 const output=await img.evaluate(n=>({width:n.naturalWidth,height:n.naturalHeight,fit:getComputedStyle(n).objectFit}));
 expect(output.fit).toBe('contain');expect(output.width/output.height).toBeCloseTo(preview.width/preview.height,1);
 await page.getByLabel('验证当前管理员密码').fill('synthetic admin password');await page.getByRole('button',{name:'保存并发布首页'}).click();
 await expect.poll(()=>saved?.announcements[0].image).toBe(src);
});


test('homepage showcase preserves a complete image on both public and signed-in pages',async({page})=>{
 const image=await picture(page);
 for(const loggedIn of [false,true]){
  await appFixture(page,{loggedIn,replies:{'/api/site':{...home,image}}});
  await page.goto('/');const img=page.getByAltText('站点展示图片');
  await expect(img).toBeVisible();await expect.poll(()=>img.evaluate(n=>n.complete&&n.naturalHeight===600)).toBe(true);
  const shape=await img.evaluate(n=>({ratio:n.clientWidth/n.clientHeight,fit:getComputedStyle(n).objectFit}));
  expect(shape.fit).toBe('contain');if(!loggedIn)expect(shape.ratio).toBeCloseTo(4/3,1);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
 }
});
test('homepage showcase offers display-frame and free crops with a matching saved preview',async({page})=>{
 const image=await picture(page);let saved;const draft={...home,announcements:[]};
 await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'admin@example.invalid',is_admin:true}},'/api/admin/home':draft}});
 await page.route('**/api/admin/home',route=>{if(route.request().method()==='POST'){saved=route.request().postDataJSON().home;return route.fulfill({json:saved});}return route.fulfill({json:draft});});
 await page.goto('/');await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'外观、协议与公告'}).click();
 await page.locator('.admin-home input[type=file]').nth(1).setInputFiles({name:'home-portrait.png',mimeType:'image/png',buffer:Buffer.from(image.split(',')[1],'base64')});
 const crop=page.locator('.image-cropper');await expect(crop.getByLabel('裁剪比例')).toHaveValue('free');
 await expect(crop.getByLabel('裁剪比例')).toContainText('首页展示框 · 4:3');
 const live=crop.getByLabel('裁剪结果实时预览');await expect(live).toBeVisible();
 expect(await live.evaluate(n=>n.width/n.height)).toBeCloseTo(.5,2);
 await crop.getByLabel('裁剪比例').selectOption('display');
 expect(await live.evaluate(n=>n.clientWidth/n.clientHeight)).toBeCloseTo(4/3,1);
 await crop.getByLabel('裁剪比例').selectOption('free');await crop.getByLabel('右下裁剪角').press('Shift+ArrowUp');
 await crop.screenshot({path:test.info().outputPath('home-crop.png')});
 await crop.getByRole('button',{name:'使用裁剪图片'}).click();
 const img=page.getByAltText('首页展示预览');await expect(img).toBeVisible();
 await expect.poll(()=>img.evaluate(n=>n.complete&&n.naturalWidth>0)).toBe(true);
 expect(await img.evaluate(n=>n.clientWidth/n.clientHeight)).toBeCloseTo(4/3,1);
 expect(await img.evaluate(n=>getComputedStyle(n).objectFit)).toBe('contain');
 await page.locator('.home-image-preview').screenshot({path:test.info().outputPath('home-saved-preview.png')});
 const src=await img.getAttribute('src');await page.getByLabel('验证当前管理员密码').fill('synthetic admin password');await page.getByRole('button',{name:'保存并发布首页'}).click();
 await expect.poll(()=>saved?.image).toBe(src);expect(saved?.site_icon).toBe('');
});


test('community homepage displays a clear separate image without overlapping welcome text',async({page})=>{
 const image=await picture(page);await appFixture(page,{replies:{'/api/site':{...home,image}}});await page.goto('/');
 const img=page.getByAltText('站点展示图片');await expect(img).toBeVisible();
 await expect(img).toHaveCSS('opacity','1');
 const imageBox=await img.boundingBox(),copyBox=await page.locator('.community-welcome-copy').boundingBox();
 expect(imageBox.x>=copyBox.x+copyBox.width-1||imageBox.y>=copyBox.y+copyBox.height-1).toBe(true);
 await page.locator('.community-welcome').screenshot({path:test.info().outputPath('community-image-card.png')});
});
test('an existing homepage image can be reopened for editing without changing announcements',async({page})=>{
 const image=await picture(page),draft={...home,image};await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'admin@example.invalid',is_admin:true}},'/api/admin/home':draft}});
 await page.goto('/');await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'外观、协议与公告'}).click();
 await page.getByRole('button',{name:'重新裁剪首页图片',exact:true}).click();await expect(page.locator('.image-cropper').getByLabel('裁剪比例')).toHaveValue('free');
 await page.locator('.image-cropper').getByRole('button',{name:'使用裁剪图片'}).click();const img=page.getByAltText('首页展示预览');await expect.poll(()=>img.evaluate(n=>n.complete&&n.naturalHeight===600)).toBe(true);
 await expect(page.getByLabel('公告标题')).toHaveValue('裁剪公告');
});


test('a valid stored image near the server size limit remains editable',async({page})=>{
 const small=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=300;c.height=600;return c.toDataURL('image/jpeg');});
 const raw=Buffer.from(small.split(',')[1],'base64'),image='data:image/jpeg;base64,'+Buffer.concat([raw,Buffer.alloc(128*1024-raw.length)]).toString('base64');
 expect(image.length).toBeGreaterThan(170000);expect(image.length).toBeLessThanOrEqual(175000);
 const draft={...home,image};await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'admin@example.invalid',is_admin:true}},'/api/admin/home':draft}});
 await page.goto('/');await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'外观、协议与公告'}).click();
 await page.getByRole('button',{name:'重新裁剪首页图片',exact:true}).click();const crop=page.locator('.image-cropper');await expect(crop).toBeVisible();await expect(crop.getByLabel('裁剪比例')).toHaveValue('free');
 await crop.getByRole('button',{name:'使用裁剪图片'}).click();await expect.poll(()=>page.getByAltText('首页展示预览').evaluate(n=>n.complete&&n.naturalHeight===600)).toBe(true);
});
