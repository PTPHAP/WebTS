import {test,expect} from '@playwright/test';
import {appFixture} from './app-fixture.mjs';

const first='00000000-0000-4000-8000-000000000001',second='00000000-0000-4000-8000-000000000002';
const metadata=(id,owner=2,kind='post',parent=null)=>({id,owner,name:`社区成员 ${owner}`,kind,parent,created_at:1,liked:false,likes:0,can_delete:owner===1});
async function feedFixture(page,{foreign=false}={}){
 const writes=[];let rows=[metadata(first)],content={[first]:{title:'周末一起玩',text:'<img src=x onerror=alert(1)>',image:''}},failed=false;
 await appFixture(page);
 await page.route('**/api/community**',async route=>{
  const req=route.request(),url=new URL(req.url()),path=url.pathname.replace('/api',''),body=req.method()==='POST'?req.postDataJSON():null;
  if(body)writes.push({path,body});
  if(path==='/community'&&body){
   if(body.payload.text==='重试内容'&&!failed){failed=true;return route.fulfill({status:503,json:{error:'模拟传输中断'}});}
   if(body.kind!=='report'){rows.unshift(metadata(body.id,1,body.kind,body.parent));content[body.id]=body.payload;}
   return route.fulfill({json:{account:1,id:body.id}});
  }
  if(path==='/community')return route.fulfill({json:{account:foreign?2:1,enabled:true,items:rows.filter(r=>r.kind===(url.searchParams.get('kind')||'post')&&r.parent===url.searchParams.get('parent')),next:null,server_time:1}});
  const id=path.split('/')[2];
  if(path.endsWith('/like')){rows=rows.map(r=>r.id===id?{...r,liked:body.liked,likes:Number(body.liked)}:r);return route.fulfill({json:{account:1}});}
  if(path.endsWith('/delete')){rows=rows.filter(r=>r.id!==id&&r.parent!==id);return route.fulfill({json:{account:1}});}
  return route.fulfill({json:{account:1,id,content:content[id],liked:rows.find(r=>r.id===id)?.liked,likes:rows.find(r=>r.id===id)?.likes}});
 });
 return writes;
}

test('member publishes, comments, likes, reports and deletes with mobile-safe layout and literal text',async({page})=>{
 const writes=await feedFixture(page);await page.goto('/community');const feed=page.getByRole('region',{name:'社区动态',exact:true});
 await expect(feed.getByText('周末一起玩',{exact:true})).toBeVisible();await expect(feed).toContainText('<img src=x onerror=alert(1)>');expect(await feed.locator('img[src=x]').count()).toBe(0);
 const picture=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=120;c.height=80;const x=c.getContext('2d');x.fillStyle='#547cff';x.fillRect(0,0,120,80);return c.toDataURL('image/png');});
 await feed.locator('.community-publish>summary').click();await feed.getByLabel('添加动态配图').setInputFiles({name:'sample.png',mimeType:'image/png',buffer:Buffer.from(picture.split(',')[1],'base64')});await page.locator('.image-cropper').getByRole('button',{name:'使用裁剪图片'}).click();await expect(feed.getByAltText('动态配图预览')).toBeVisible();
 await feed.getByLabel('动态标题').fill('新动态');await feed.getByLabel('动态正文').fill('服务器加密保存到对象存储');await feed.getByRole('button',{name:'发布动态',exact:true}).click();
 await expect(feed.locator('.feed-card').filter({hasText:'新动态'})).toBeVisible();const sent=writes.find(v=>v.body.kind==='post').body;
 expect(sent.payload.title).toBe('新动态');expect(sent.payload.image).toMatch(/^data:image\/jpeg;base64,/);expect(sent.id).toMatch(/^[\da-f-]{36}$/);expect(sent).not.toHaveProperty('text');
 let card=feed.locator('.feed-card').filter({hasText:'周末一起玩'});await card.getByRole('button',{name:'赞 · 0',exact:true}).click();await expect(feed.getByRole('button',{name:'赞 · 1',exact:true})).toBeVisible();
 card=feed.locator('.feed-card').filter({hasText:'周末一起玩'});await card.getByRole('button',{name:'举报',exact:true}).click();await card.getByLabel('举报原因').fill('合成举报，仅测试流程');await card.getByRole('button',{name:'提交举报',exact:true}).click();await expect(card).toContainText('举报已提交');expect(writes.at(-1).body.kind).toBe('report');
 await card.getByRole('button',{name:'查看讨论',exact:true}).click();await feed.getByLabel('评论内容').fill('收到，周末见');await feed.getByRole('button',{name:'发表评论',exact:true}).click();await expect(feed).toContainText('收到，周末见');expect(writes.at(-1).body.parent).toBe(first);
 const root=feed.locator('.feed-card').filter({hasText:'周末一起玩'});await expect(root.getByRole('button',{name:'赞 · 1',exact:true})).toHaveAttribute('aria-pressed','true');await root.getByRole('button',{name:'赞 · 1',exact:true}).click();await expect(feed.locator('.feed-card').filter({hasText:'周末一起玩'}).getByRole('button',{name:'赞 · 0',exact:true})).toHaveAttribute('aria-pressed','false');
 await feed.getByRole('button',{name:'返回动态'}).click();
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});await feed.scrollIntoViewIfNeeded();expect(await feed.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);}
 await page.setViewportSize({width:390,height:844});await feed.scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('community-feed-mobile.png')});
 page.on('dialog',d=>d.accept());await feed.locator('.feed-card').filter({hasText:'新动态'}).getByRole('button',{name:'删除',exact:true}).click();await expect(feed.locator('.feed-card').filter({hasText:'新动态'})).toHaveCount(0);
});

test('failed publish retries its original id and account-mismatched feeds fail closed',async({page})=>{
 const writes=await feedFixture(page);await page.goto('/community');const feed=page.getByRole('region',{name:'社区动态',exact:true});await feed.locator('.community-publish>summary').click();await feed.getByLabel('动态正文').fill('重试内容');await feed.getByRole('button',{name:'发布动态',exact:true}).click();await expect(feed.getByRole('alert')).toContainText('模拟传输中断');await feed.getByRole('button',{name:'发布动态',exact:true}).click();await expect(feed.locator('.feed-card').filter({hasText:'重试内容'})).toBeVisible();expect(writes[0].body).toEqual(writes[1].body);
 await page.route('**/api/community?**',route=>route.fulfill({json:{account:2,enabled:true,items:[metadata(second)],next:null,server_time:1}}));await feed.getByRole('button',{name:'刷新',exact:true}).click();await expect(feed.getByRole('alert')).toContainText('账号已变化');await expect(feed.locator('.feed-card')).toHaveCount(0);await expect(feed.locator('.community-publish')).toHaveCount(0);
});

test('content fetches have at most two workers and stop when home is unmounted',async({page})=>{
 let active=0,peak=0,gets=0;const rows=[1,2,3,4,5].map(n=>metadata(`00000000-0000-4000-8000-${String(n).padStart(12,'0')}`));
 await appFixture(page,{replies:{'/api/community':{account:1,enabled:true,items:rows,next:null,server_time:1}}});
 await page.route(/\/api\/community\/[\da-f-]+\?/,async route=>{active++;gets++;peak=Math.max(peak,active);await new Promise(r=>setTimeout(r,180));active--;try{await route.fulfill({json:{account:1,id:new URL(route.request().url()).pathname.split('/').at(-1),content:{title:'合成内容',text:'测试',image:''}}});}catch{}});
 await page.goto('/community');await expect.poll(()=>gets).toBeGreaterThanOrEqual(2);await page.getByRole('button',{name:'语音空间',exact:true}).click();const stopped=gets;await page.waitForTimeout(500);expect(peak).toBeLessThanOrEqual(2);expect(gets).toBe(stopped);expect(await page.locator('.community-feed').count()).toBe(0);
});

test('admin sees and resolves report queue while member publishing is disabled',async({page})=>{
 const writes=[];let pending=true;
 await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'admin@example.invalid',is_admin:true}}}});
 await page.route('**/api/admin/community',route=>route.fulfill({json:{account:1,enabled:false,stats:{total:28000,limit:30000,posts:20,comments:30,reports:1,uploads:0,cleanup:2,retries:1,storage_configured:true}}}));
 await page.route('**/api/community**',route=>{const url=new URL(route.request().url());if(url.pathname==='/api/community')return route.fulfill({json:{account:1,enabled:true,items:pending?[metadata(second,2,'report',first)]:[],next:null,server_time:1}});const id=url.pathname.split('/').at(-1);return route.fulfill({json:{account:1,id,content:{title:id===first?'举报目标':'',text:id===first?'目标正文':'合成举报原因',image:''}}});});
 await page.route(`**/api/admin/community/${second}`,route=>{writes.push(route.request().postDataJSON());pending=false;return route.fulfill({json:{account:1}});});
 await page.goto('/');await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'社区动态',exact:true}).click();const feed=page.getByRole('region',{name:'社区内容管理'});await expect(feed).toContainText('合成举报原因');await expect(feed.getByRole('complementary',{name:'社区运行概览'})).toContainText('28000 / 30000');await expect(feed).toContainText('存在删除失败对象');await page.getByRole('button',{name:'刷新概览',exact:true}).click();await expect(feed).toContainText('合成举报原因');await feed.getByRole('button',{name:'查看举报目标'}).click();await expect(feed).toContainText('目标正文');await feed.getByRole('button',{name:'不予处理并结案'}).click();await expect(feed).toContainText('暂无待处理举报');expect(writes).toEqual([{account:1,remove:false}]);for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await feed.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);}await page.setViewportSize({width:390,height:844});await feed.getByRole('complementary',{name:'社区运行概览'}).scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('community-overview-mobile.png')});
});


test('categories filter old posts, keep read-only history and submit a selected game category',async({page})=>{
 const game='00000000-0000-4000-8000-000000000099',archive='00000000-0000-4000-8000-000000000098';
 const categories=[{id:'general',name:'综合',enabled:true},{id:game,name:'CS2',enabled:true},{id:archive,name:'旧赛季'+'x'.repeat(55),enabled:false}],writes=[],queries=[];
 let rows=[{...metadata(first),category:'general'},{...metadata(second),category:archive}],contents={[first]:{title:'旧帖子',text:'迁移前的内容',image:''},[second]:{title:'历史战报',text:'保留只读分区',image:''}};
 await appFixture(page);await page.route('**/api/community**',route=>{const req=route.request(),u=new URL(req.url());
  if(req.method()==='POST'){const b=req.postDataJSON();writes.push(b);rows.unshift({...metadata(b.id,1,b.kind,b.parent),category:b.category});contents[b.id]=b.payload;return route.fulfill({json:{account:1,id:b.id}});}
  if(u.pathname==='/api/community'){queries.push(u.search);return route.fulfill({json:{account:1,enabled:true,categories,items:rows.filter(r=>r.kind===(u.searchParams.get('kind')||'post')&&r.parent===u.searchParams.get('parent')&&(!u.searchParams.has('category')||r.category===u.searchParams.get('category'))),next:null,server_time:1}});}
  const id=u.pathname.split('/').at(-1);return route.fulfill({json:{account:1,id,content:contents[id]}});
 });
 await page.goto('/community');const feed=page.getByRole('region',{name:'社区动态',exact:true}),nav=feed.getByRole('navigation',{name:'社区分区'});
 await expect(feed).toContainText('旧帖子');await nav.getByRole('button',{name:'旧赛季'}).click();await expect(feed).toContainText('历史战报');await expect(feed).not.toContainText('迁移前的内容');await page.setViewportSize({width:320,height:844});expect(await feed.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);await page.setViewportSize({width:1280,height:844});
 await feed.locator('.community-publish>summary').click();await expect(feed.getByLabel('发布到分区').locator('option')).toHaveCount(2);await expect(feed.getByLabel('发布到分区')).toHaveValue('general');
 await nav.getByRole('button',{name:'CS2',exact:true}).click();await expect(nav.getByRole('button',{name:'CS2',exact:true})).toHaveAttribute('aria-pressed','true');await feed.locator('.community-publish>summary').click();await expect(feed.getByLabel('发布到分区')).toHaveValue(game);
 await feed.getByLabel('动态正文').fill('新分区分享');await feed.getByRole('button',{name:'发布动态',exact:true}).click();await expect(feed).toContainText('新分区分享');expect(writes[0].category).toBe(game);
 await feed.getByRole('button',{name:'查看讨论',exact:true}).click();await expect(feed).toContainText('帖子讨论');expect(new URLSearchParams(queries.at(-1)).has('category')).toBe(false);await feed.getByRole('button',{name:'返回动态'}).click();await expect(nav.getByRole('button',{name:'CS2',exact:true})).toHaveAttribute('aria-pressed','true');
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);expect(await feed.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);}
 await page.setViewportSize({width:390,height:844});await feed.scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('categories-mobile.png'),animations:'disabled'});
});

test('admin category editor saves order, disables new posting and retains edits on revision conflict',async({page})=>{
 let categories=[{id:'general',name:'综合',enabled:true}],revision=0;const writes=[];
 await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'admin@example.invalid',is_admin:true}}}});
 await page.route('**/api/admin/community',route=>route.fulfill({json:{account:1,enabled:false,categories,category_revision:revision,stats:{total:0,limit:30000,posts:0,comments:0,reports:0,uploads:0,cleanup:0,retries:0,storage_configured:true}}}));
 await page.route('**/api/admin/community/categories',route=>{const b=route.request().postDataJSON();writes.push(b);if(writes.length===1)return route.fulfill({status:409,json:{error:'分区已被修改，请刷新后重新编辑'}});categories=b.categories;revision++;return route.fulfill({json:{account:1,revision}});});
 await page.goto('/');await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'社区动态',exact:true}).click();const editor=page.locator('.community-category-editor');await editor.locator('summary').click();await editor.getByRole('button',{name:'添加分区',exact:true}).click();await editor.getByLabel('分区名称 2').fill('CS2');await editor.getByLabel('保存分区时验证管理员密码').fill('synthetic');await editor.getByRole('button',{name:'保存分区',exact:true}).click();await expect(editor.getByRole('alert')).toContainText('分区已被修改');await expect(editor.getByLabel('分区名称 2')).toHaveValue('CS2');await expect(editor.getByLabel('保存分区时验证管理员密码')).toHaveValue('');
 await editor.getByLabel('接收新帖').nth(1).uncheck();await editor.getByRole('button',{name:'上移分区 2'}).click();await expect(editor.getByLabel('分区名称 1')).toHaveValue('CS2');await editor.getByLabel('保存分区时验证管理员密码').fill('synthetic');await Promise.all([page.waitForResponse(r=>new URL(r.url()).pathname==='/api/admin/community'&&r.request().method()==='GET'),editor.getByRole('button',{name:'保存分区',exact:true}).click()]);await editor.locator('summary').click();await expect(editor.getByLabel('分区名称 1')).toHaveValue('CS2');expect(writes.at(-1).categories[0]).toMatchObject({name:'CS2',enabled:false});await expect(editor.getByLabel('接收新帖').nth(1)).toBeDisabled();expect(await editor.locator('.category-enabled').first().evaluate(n=>getComputedStyle(n).flexDirection)).toBe('row');
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await editor.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);}
 await page.setViewportSize({width:390,height:844});await editor.scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('category-editor-mobile.png'),animations:'disabled'});
});


test('community directory, readable topic previews and keyboard writing work across layouts',async({page})=>{
 const game='00000000-0000-4000-8000-000000000099';
 const categories=[{id:'general',name:'综合',enabled:true},{id:game,name:'CS2',enabled:true},...Array.from({length:6},(_,i)=>({id:`00000000-0000-4000-8000-${String(200+i).padStart(12,'0')}`,name:i===0?'一起发现游戏里的那些美好瞬间':'讨论分区 '+i,enabled:i!==5}))];
 const rows=[{...metadata(first),name:'旅人',category:game},{...metadata(second),name:'夏末',category:'general'}],body='昨晚一起看完了比赛，最后一回合实在太精彩了。一起聊聊你印象最深的瞬间吧。'.repeat(12);
 const picture=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=320;c.height=200;const x=c.getContext('2d');x.fillStyle='#a5c0ef';x.fillRect(0,0,320,200);x.fillStyle='#3f64a3';x.fillRect(40,50,90,100);x.fillStyle='#f0d9b5';x.beginPath();x.arc(220,90,50,0,Math.PI*2);x.fill();return c.toDataURL('image/jpeg');});
 await appFixture(page,{replies:{'/api/community':{account:1,enabled:true,categories,items:rows,next:null,server_time:1}}});
 const preferences={};await page.route('**/api/preferences',route=>{if(route.request().method()==='PATCH')Object.assign(preferences,route.request().postDataJSON().patch);return route.fulfill({json:{account:1,initialized:true,preferences}});});
 await page.route(/\/api\/community\/[\da-f-]+\?/,route=>{const id=new URL(route.request().url()).pathname.split('/').at(-1);return route.fulfill({json:{account:1,id,content:{title:id===first?'昨晚的决胜局，你看了吗？':'找到一款适合周末一起玩的游戏',text:body,image:id===first?picture:''}}});});
 await page.setViewportSize({width:1280,height:900});await page.goto('/community');const feed=page.getByRole('region',{name:'社区动态',exact:true});await expect(feed.getByRole('button',{name:'昨晚的决胜局，你看了吗？',exact:true})).toBeVisible();
 const directory=page.locator('.community-directory'),timeline=page.locator('.community-timeline');expect((await directory.boundingBox()).x+(await directory.boundingBox()).width).toBeLessThan((await timeline.boundingBox()).x);
 await expect(feed.locator('.feed-card[data-preview=true]')).toHaveCount(2);expect(await feed.locator('.feed-card').first().locator('.feed-content>p').evaluate(n=>getComputedStyle(n).webkitLineClamp)).toBe('3');
 expect((await feed.getByRole('button',{name:'写点什么'}).boundingBox()).height).toBeLessThanOrEqual(48);await feed.getByRole('button',{name:'写点什么'}).focus();await page.keyboard.press('Enter');await expect(feed.getByLabel('动态标题')).toBeFocused();await expect(feed.getByLabel('动态正文')).toBeVisible();await feed.locator('.community-publish>summary').click();
 await page.locator('.community-hub').screenshot({path:test.info().outputPath('community-desktop-dark.png'),animations:'disabled'});
 await Promise.all([page.waitForResponse(r=>new URL(r.url()).pathname==='/api/preferences'&&r.request().method()==='PATCH'),page.getByRole('button',{name:'切换深浅主题'}).click()]);await expect(page.locator('html')).toHaveAttribute('data-theme','light');await page.locator('.community-hub').screenshot({path:test.info().outputPath('community-desktop-light.png'),animations:'disabled'});
 for(const width of [320,390,760,980,1280]){await page.setViewportSize({width,height:900});expect((await feed.getByRole('button',{name:'写点什么'}).boundingBox()).height).toBeLessThanOrEqual(48);expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);expect(await feed.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);}
 await page.setViewportSize({width:390,height:844});const nav=feed.getByRole('navigation',{name:'社区分区'});expect(await nav.evaluate(n=>n.scrollWidth>n.clientWidth)).toBe(true);await page.locator('.community-hub').scrollIntoViewIfNeeded();await expect(page.locator('html')).toHaveAttribute('data-theme','light');await page.screenshot({path:test.info().outputPath('community-mobile-light.png'),animations:'disabled'});
 await page.emulateMedia({reducedMotion:'reduce'});await feed.getByRole('button',{name:'写点什么'}).click();await expect(feed.getByLabel('动态标题')).toBeFocused();await expect(feed.getByLabel('动态正文')).toBeVisible();expect(await feed.locator('.feed-composer').evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);expect(await feed.locator('.feed-card').first().evaluate(n=>getComputedStyle(n).animationName)).toBe('none');await feed.locator('.community-publish>summary').click();
 await feed.getByRole('button',{name:'昨晚的决胜局，你看了吗？',exact:true}).click();await expect(feed.getByRole('heading',{name:'帖子讨论',exact:true})).toBeVisible();await expect(feed.locator('.feed-card').first()).toHaveAttribute('data-preview','false');expect(await feed.locator('.feed-card').first().locator('.feed-content>p').evaluate(n=>getComputedStyle(n).webkitLineClamp)).toBe('none');
});


test('community shows real avatars in summaries posts and comments and updates own profile in place',async({page})=>{
 const comment='00000000-0000-4000-8000-000000000003',hash='a'.repeat(64),reads=[];
 const png=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=c.height=64;const x=c.getContext('2d');x.fillStyle='#6389d8';x.fillRect(0,0,64,64);x.fillStyle='#f7dfaf';x.beginPath();x.arc(32,25,17,0,Math.PI*2);x.fill();return c.toDataURL('image/png').split(',')[1];});
 let profile={display_name:'自己的社区头像',avatar:png,about:'',sync_avatar:false,sync_about:false};
 await appFixture(page);
 await page.route('**/api/profile',route=>{if(route.request().method()==='POST')profile=route.request().postDataJSON();return route.fulfill({json:profile});});
 await page.route('**/api/community?**',route=>{const u=new URL(route.request().url()),comments=u.searchParams.get('kind')==='comment';return route.fulfill({json:{account:1,enabled:true,items:comments?[{...metadata(comment,2,'comment',first),avatar_hash:hash}]:[{...metadata(first),avatar_hash:hash},{...metadata(second,1),avatar_hash:hash}],next:null,server_time:1}});});
 await page.route(/\/api\/community\/[\da-f-]+\?/,route=>{const id=new URL(route.request().url()).pathname.split('/').at(-1);return route.fulfill({json:{account:1,id,content:{title:id===comment?'':'社区头像展示',text:'合成头像验证，没有真实账号数据。',image:''},name:id===second?'旧昵称':'社区成员 2',avatar_hash:hash}});});
 await page.route(/\/api\/community\/[\da-f-]+\/avatar\?/,route=>{reads.push(route.request().url());return route.fulfill({contentType:'image/png',headers:{'cache-control':'private, no-store'},body:Buffer.from(png,'base64')});});
 await page.goto('/');let avatars=page.locator('.community-latest .community-author-avatar img');await expect(avatars).toHaveCount(2);await expect.poll(()=>avatars.first().evaluate(n=>n.complete&&n.naturalWidth>0)).toBe(true);
 await page.getByRole('button',{name:'进入社区 ↗',exact:true}).click();const feed=page.getByRole('region',{name:'社区动态',exact:true});await expect(feed.locator('.community-author-avatar img')).toHaveCount(2);const mine=feed.locator('.feed-card').filter({hasText:'自己的社区头像'});await expect(mine.locator('.community-author-avatar img')).toHaveAttribute('src',`data:image/png;base64,${png}`);
 await feed.locator('.community-publish>summary').click();await feed.getByLabel('动态正文').fill('尚未发送的草稿');
 await page.getByRole('button',{name:'账号与个人资料'}).click();await page.getByRole('button',{name:'个人资料 · 昵称与固定头像'}).click();const form=page.locator('.profile-settings');await form.getByLabel('社区昵称',{exact:true}).fill('更改后的社区昵称');await form.getByRole('button',{name:'移除账号默认头像'}).click();await form.getByRole('button',{name:'保存个人资料',exact:true}).click();await expect(form).toContainText('资料已保存');await page.getByRole('dialog').getByRole('button',{name:'关闭',exact:true}).click();
 await expect(feed.getByLabel('动态正文')).toHaveValue('尚未发送的草稿');const updated=feed.locator('.feed-card').filter({hasText:'更改后的社区昵称'});await expect(updated.locator('.community-author-avatar img')).toHaveCount(0);await expect(updated.locator('.community-author-avatar')).toContainText('更');
 await feed.locator('.feed-card').filter({hasText:'社区成员 2'}).getByRole('button',{name:'查看讨论'}).click();await expect(feed.locator('.community-author-avatar img')).toHaveCount(2);await expect.poll(()=>feed.locator('.community-author-avatar img').last().evaluate(n=>n.complete&&n.naturalWidth>0)).toBe(true);const rootBox=await feed.locator('.feed-card').first().boundingBox(),replyBox=await feed.locator('.feed-list .feed-card').first().boundingBox();expect(replyBox.y-rootBox.y-rootBox.height).toBeGreaterThanOrEqual(12);
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);const shape=await feed.locator('.community-author-avatar').first().evaluate(n=>{const r=n.getBoundingClientRect();return {w:r.width,h:r.height,radius:getComputedStyle(n).borderRadius};});expect(Math.abs(shape.w-shape.h)).toBeLessThan(1);expect(shape.radius).toBe('50%');}
 await page.setViewportSize({width:390,height:844});await feed.scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('community-avatars-mobile.png'),animations:'disabled'});expect(reads.every(u=>new URL(u).searchParams.get('account')==='1')).toBe(true);
});

test('failed community avatar falls back cleanly and a new version loads after refresh',async({page})=>{
 const png=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=c.height=8;return c.toDataURL('image/png').split(',')[1];});let hash='a'.repeat(64);const versions=[];
 await appFixture(page);await page.route('**/api/community?**',r=>r.fulfill({json:{account:1,enabled:true,items:[{...metadata(first),avatar_hash:hash}],next:null,server_time:1}}));await page.route(/\/api\/community\/[\da-f-]+\?/,r=>r.fulfill({json:{account:1,id:first,content:{title:'版本变化测试',text:'内容正常',image:''}}}));
 await page.route(/\/api\/community\/[\da-f-]+\/avatar\?/,r=>{const v=new URL(r.request().url()).searchParams.get('v');versions.push(v);return v==='a'.repeat(64)?r.fulfill({status:409,json:{error:'头像已更新'}}):r.fulfill({contentType:'image/png',body:Buffer.from(png,'base64')});});
 await page.goto('/community');const feed=page.getByRole('region',{name:'社区动态',exact:true}),avatar=feed.locator('.community-author-avatar');await expect(avatar).toContainText('社');await expect(avatar.locator('img')).toHaveCount(0);
 hash='b'.repeat(64);await feed.getByRole('button',{name:'刷新',exact:true}).click();await expect.poll(()=>avatar.locator('img').evaluate(n=>n.complete&&n.naturalWidth>0)).toBe(true);expect(versions).toContain('a'.repeat(64));expect(versions).toContain(hash);
});

test('authors retry edits safely and admin pin/lock controls preserve readable discussions',async({page})=>{
 let state={revision:0,edited_at:0,pinned:false,locked:false},content={title:'可编辑的帖子',text:'原正文',image:''},failed=false;const writes=[];
 await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'admin@example.invalid',is_admin:true}}}});
 await page.route('**/api/community**',route=>{const r=route.request(),u=new URL(r.url());if(u.pathname==='/api/community')return route.fulfill({json:{account:1,enabled:true,items:u.searchParams.get('kind')==='comment'?[]:[{...metadata(first,1),state,can_moderate:true}],next:null,server_time:1}});
 if(u.pathname.endsWith('/edit')){const b=r.postDataJSON();writes.push(b);if(!failed){failed=true;return route.fulfill({status:503,json:{error:'模拟上传失败'}});}content=b.payload;state={...state,revision:state.revision+1,edited_at:123};return route.fulfill({json:{account:1,id:first,request_id:b.id,revision:state.revision}});}
 return route.fulfill({json:{account:1,id:first,content,state}});});
 await page.route(`**/api/admin/community/${first}/control`,route=>{const b=route.request().postDataJSON();expect(b.revision).toBe(state.revision);state={...state,pinned:b.pinned,locked:b.locked,revision:state.revision+1};return route.fulfill({json:{account:1,id:first,revision:state.revision}});});
 await page.goto('/community');let card=page.locator('.feed-card').filter({hasText:'可编辑的帖子'});await card.getByRole('button',{name:'编辑',exact:true}).click();await card.getByLabel('编辑标题').fill('更新后的帖子');await card.getByLabel('编辑正文').fill('hello <script>alert(1)</script>');await card.getByRole('button',{name:'保存编辑',exact:true}).click();await expect(card.getByRole('alert')).toContainText('模拟上传失败');await expect(card.getByLabel('编辑标题')).toHaveValue('更新后的帖子');await card.getByRole('button',{name:'保存编辑',exact:true}).click();expect(writes[0]).toEqual(writes[1]);
 card=page.locator('.feed-card').filter({hasText:'更新后的帖子'});await expect(card).toContainText('已编辑');await expect(card).toContainText('<script>alert(1)</script>');expect(await card.locator('script').count()).toBe(0);await card.getByRole('button',{name:'置顶',exact:true}).click();await expect(card.getByRole('button',{name:'取消置顶',exact:true})).toBeVisible();await card.getByRole('button',{name:'锁帖',exact:true}).click();await expect(card).toContainText('已锁定');
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});await expect(card).toBeVisible();expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);}
 await page.screenshot({path:test.info().outputPath('post-maintenance.png'),animations:'disabled'});
});

test('locked threads hide member composer and category icons crop save and render on mobile',async({page})=>{
 let revision=0,categories=[{id:'general',name:'综合',enabled:true}],image='';const writes=[];
 await appFixture(page,{replies:{'/api/me':{user:{id:1,email:'admin@example.invalid',is_admin:true}}}});
 await page.route('**/api/community**',route=>{const u=new URL(route.request().url());if(u.pathname.includes('/categories/'))return route.fulfill({contentType:'image/jpeg',body:Buffer.from(image.split(',')[1]||'','base64')});if(u.pathname==='/api/community')return route.fulfill({json:{account:1,enabled:true,categories,items:u.searchParams.get('kind')==='comment'?[]:[{...metadata(first),state:{revision:1,edited_at:0,pinned:false,locked:true}}],next:null,server_time:1}});return route.fulfill({json:{account:1,id:first,content:{title:'锁定帖',text:'继续可读',image:''},state:{revision:1,edited_at:0,pinned:false,locked:true}}});});
 await page.route('**/api/admin/community',route=>route.fulfill({json:{account:1,enabled:true,categories,category_revision:revision,stats:{total:1,limit:30000,posts:1,comments:0,reports:0,uploads:0,cleanup:0,retries:0,storage_configured:true}}}));
 await page.route('**/api/admin/community/categories/general/icon',route=>{const b=route.request().postDataJSON();writes.push(b);image=b.payload.image;categories=[{...categories[0],icon:image?b.id:undefined}];revision++;return route.fulfill({json:{account:1,id:'general',request_id:b.id,revision}});});
 await page.goto('/community');await page.getByRole('button',{name:'查看讨论',exact:true}).click();await expect(page.locator('.community-lock-note')).toContainText('帖子已锁定');await expect(page.getByLabel('评论内容')).toHaveCount(0);
 await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'社区动态',exact:true}).click();const editor=page.locator('.community-category-editor');await editor.locator('summary').click();await editor.getByRole('button',{name:'更换图标',exact:true}).click();
 const picture=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=120;c.height=120;const x=c.getContext('2d');x.fillStyle='#3d78e2';x.fillRect(0,0,120,120);return c.toDataURL('image/png');});
 await editor.getByLabel('选择综合分区图标').setInputFiles({name:'board.png',mimeType:'image/png',buffer:Buffer.from(picture.split(',')[1],'base64')});await page.locator('.image-cropper').getByLabel('裁剪比例').selectOption('display');await expect(page.locator('.image-cropper').getByRole('option',{name:/1:1/})).toHaveCount(1);await page.locator('.image-cropper').getByRole('button',{name:'使用裁剪图片'}).click();await expect(editor.getByAltText('分区图标裁剪预览')).toBeVisible();await editor.getByLabel('验证图标管理员密码').fill('synthetic board password');await editor.getByRole('button',{name:'保存图标',exact:true}).click();await editor.locator('summary').click();await expect(editor.locator('.community-category-icon img')).toBeVisible();await expect.poll(()=>editor.locator('.community-category-icon img').evaluate(n=>n.complete&&n.naturalWidth>0)).toBe(true);expect(writes[0].payload.image).toMatch(/^data:image\/jpeg;base64,/);expect(writes[0].revision).toBe(0);
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await editor.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);}
 await page.setViewportSize({width:390,height:844});await editor.scrollIntoViewIfNeeded();await page.screenshot({path:test.info().outputPath('board-icon-active-mobile.png'),animations:'disabled'});await editor.getByRole('button',{name:'更换图标',exact:true}).click();await editor.getByRole('button',{name:'恢复文字图标',exact:true}).click();await editor.getByLabel('验证图标管理员密码').fill('synthetic board password');await editor.getByRole('button',{name:'保存图标',exact:true}).click();expect(writes.at(-1).payload.image).toBe('');await editor.locator('summary').click();await expect(editor.locator('.community-category-icon img')).toHaveCount(0);await page.screenshot({path:test.info().outputPath('board-icon-mobile.png'),animations:'disabled'});
});

test('author history is a bounded public-community filter and discussion navigation keeps its scope',async({page})=>{
 const queries=[];const row={...metadata(first),comments:2},other={...metadata(second,3),comments:0};
 await appFixture(page);
 await page.route('**/api/community**',route=>{const u=new URL(route.request().url());if(u.pathname==='/api/community'){queries.push(u.search);const filter=u.searchParams.get('author');const comment=u.searchParams.get('kind')==='comment';return route.fulfill({json:{account:1,enabled:true,author:filter?{owner:2,name:'社区成员 2',avatar_hash:''}:null,items:comment?[]:filter?[row]:[row,other],next:null,server_time:1}});}const id=u.pathname.split('/').at(-1);return route.fulfill({json:{account:1,id,content:{title:id===first?'作者的分享':'其他作者',text:'当前可读内容',image:''},comments:id===first?2:0}});});
 await page.goto('/community');await expect(page.getByRole('button',{name:'讨论 · 2',exact:true})).toBeVisible();await page.getByRole('button',{name:'社区成员 2',exact:true}).click();await expect(page.getByRole('heading',{name:'社区成员 2的分享',exact:true})).toBeVisible();await expect(page.getByText('其他作者',{exact:true})).toHaveCount(0);await expect(page.locator('.community-publish')).toHaveCount(0);expect(queries.at(-1)).toContain('author='+first);
 await page.getByRole('button',{name:'讨论 · 2',exact:true}).click();await expect(page.getByLabel('评论内容')).toBeVisible();expect(queries.at(-1)).not.toContain('author=');await page.getByRole('button',{name:'返回动态',exact:true}).click();await expect(page.getByRole('heading',{name:'社区成员 2的分享',exact:true})).toBeVisible();await page.getByRole('button',{name:'全部动态',exact:true}).click();await expect(page.getByText('其他作者',{exact:true})).toBeVisible();
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);}
});

test('community replies share inbox polling, preserve read snapshots and open the correct discussion',async({page})=>{
 const writes=[];let notices=0,replyGets=0,discussionLoads=0,summary={enabled:true,unread:1,latest:2},through=2;
 const root={...metadata(first,1),state:{revision:0,edited_at:0,pinned:false,locked:false}};
 let replies=[{seq:2,id:second,actor:'社区回复者 <script>',created_at:1,read:false,post:root}];
 await appFixture(page);
 await page.route('**/api/notices',route=>{notices++;return route.fulfill({json:{account:1,items:[],unread:0,email_enabled:false,community:summary}});});
 await page.route('**/api/community/replies**',route=>{const req=route.request(),u=new URL(req.url());if(u.pathname.endsWith('/read')){const b=req.postDataJSON();writes.push(b);summary={enabled:true,unread:1,latest:3};return route.fulfill({json:{account:1,summary}});}replyGets++;return route.fulfill({json:{account:1,enabled:true,items:replies,next:null,through,summary}});});
 await page.route('**/api/community?**',route=>{const u=new URL(route.request().url());if(u.searchParams.get('kind')==='comment')discussionLoads++;return route.fulfill({json:{account:1,enabled:true,items:u.searchParams.get('kind')==='comment'?[]:[root],next:null,server_time:1}});});
 await page.route(`**/api/community/${first}?**`,route=>route.fulfill({json:{account:1,id:first,content:{title:'我的原帖',text:'已定位到原来的讨论',image:''},state:root.state}}));
 await page.goto('/');await expect(page.getByRole('button',{name:'站点信箱，1条社区回复未读',exact:true})).toBeVisible();expect(replyGets).toBe(0);await expect(page.locator('.activity-alert')).toHaveCount(0);
 await page.getByRole('button',{name:/站点信箱/}).click();const inbox=page.getByRole('dialog');await expect(inbox.getByRole('region',{name:'社区回复通知'})).toContainText('社区回复者 <script>');expect(await inbox.locator('script').count()).toBe(0);await inbox.locator('.community-reply-row').click();await expect(page.getByRole('heading',{name:'我的原帖',exact:true})).toBeVisible();await expect(page.getByLabel('评论内容')).toBeVisible();await expect(page.getByRole('dialog')).toHaveCount(0);
 await page.getByRole('button',{name:/站点信箱/}).click();await inbox.getByRole('button',{name:'全部标记已读',exact:true}).click();expect(writes[0]).toEqual({account:1,through:2});await expect(inbox).toContainText('1 条未读');await expect(inbox.locator('.community-reply-row')).not.toHaveClass(/unread/);
 through=3;replies=[{seq:3,id:'00000000-0000-4000-8000-000000000003',actor:'新回复',created_at:2,read:false,post:root},...replies.map(v=>({...v,read:true}))];await inbox.getByRole('button',{name:'刷新回复',exact:true}).click();await expect(inbox).toContainText('新回复');expect(replyGets).toBe(3);
 for(const width of [320,390,1280]){await page.setViewportSize({width,height:844});expect(await inbox.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);}
 await page.setViewportSize({width:390,height:844});await page.screenshot({path:test.info().outputPath('community-replies-mobile.png'),animations:'disabled'});expect(notices).toBeGreaterThanOrEqual(1);await inbox.locator('.community-reply-row').filter({hasText:'新回复'}).click();await expect.poll(()=>discussionLoads).toBe(2);
});

test('reply inbox fails closed on foreign accounts and new reply alerts obey quiet preferences',async({page})=>{
 await page.clock.install();let summary={enabled:true,unread:0,latest:0},foreign=true;
 await appFixture(page,{replies:{'/api/preferences':{account:1,initialized:true,preferences:{notifications:{quiet:true,messages:true,poke:true,friends:true,mail:true,activities:true,channel:true}}}}});
 await page.route('**/api/notices',route=>route.fulfill({json:{account:1,items:[],unread:0,email_enabled:false,community:summary}}));
 await page.route('**/api/community/replies?**',route=>route.fulfill({json:{account:foreign?2:1,enabled:true,items:[{seq:1,id:second,actor:'不可泄漏',created_at:1,read:false,post:metadata(first,1)}],through:1,next:null,summary}}));
 await page.goto('/');await expect(page.getByRole('button',{name:'通知设置 · 免打扰已开启'})).toBeVisible();summary={enabled:true,unread:1,latest:1};await page.clock.fastForward(20001);await expect(page.getByRole('button',{name:'站点信箱，1条社区回复未读',exact:true})).toBeVisible();await expect(page.locator('.activity-alert')).toHaveCount(0);
 await page.getByRole('button',{name:/站点信箱/}).click();const inbox=page.getByRole('dialog');await expect(inbox.getByRole('alert')).toContainText('账号已变化');await expect(inbox).not.toContainText('不可泄漏');await expect(inbox.locator('.community-reply-row')).toHaveCount(0);foreign=false;await inbox.getByRole('button',{name:'刷新回复',exact:true}).click();await expect(inbox).toContainText('不可泄漏');
});

test('new community reply alerts appear once and never copy reply bodies into notifications',async({page})=>{
 await page.clock.install();let community={enabled:true,unread:0,latest:0};await appFixture(page);await page.route('**/api/notices',route=>route.fulfill({json:{account:1,items:[],unread:0,email_enabled:false,community}}));await page.goto('/');await expect(page.getByRole('button',{name:'站点信箱',exact:true})).toBeVisible();await page.getByRole('button',{name:'通知设置',exact:true}).click();const settings=page.getByRole('dialog',{name:'通知与免打扰'});await expect(settings.getByLabel('聊天、私信与社区回复')).toBeEnabled();await settings.getByRole('button',{name:'关闭',exact:true}).click();await page.getByRole('button',{name:'站点信箱',exact:true}).click();await expect(page.getByRole('dialog').getByRole('button',{name:'社区回复',exact:true})).toBeVisible();await page.getByRole('button',{name:'关闭信箱'}).click();community={enabled:true,unread:1,latest:10};await page.clock.fastForward(20001);await expect(page.locator('.activity-alert')).toContainText('收到新的社区回复');await expect(page.locator('.activity-alert')).toContainText('点击右上角信箱');await expect(page.getByRole('button',{name:'站点信箱，1条社区回复未读',exact:true})).toBeVisible();await page.clock.fastForward(20001);await expect(page.locator('.activity-alert')).toHaveCount(0);community={enabled:true,unread:2,latest:11};await page.clock.fastForward(20001);await expect(page.locator('.activity-alert')).toContainText('收到新的社区回复');
});
