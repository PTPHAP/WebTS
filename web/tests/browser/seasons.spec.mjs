import {test,expect} from '@playwright/test';
import {appFixture} from './app-fixture.mjs';
const base={season:'winter',site_name:'WebTS',site_icon:'',footer_html:'',operator:'',contact:'',data_details:'',privacy_policy:'',terms:'',title:'属于我们的季节',subtitle:'仅使用合成数据验收',image:'',announcements:[]};
async function fixture(page,{admin=false,watch=false,voice=false}={}){
  let home={...base},preferences={theme:'dark',seasonPreference:'follow'};const writes=[];
  await appFixture(page,{watch,replies:{'/api/site':home,'/api/preferences':{account:1,initialized:true,preferences},...(admin?{'/api/me':{user:{id:1,email:'theme-admin@example.invalid',is_admin:true}}}:{}),...(voice?{'/api/identities':{identities:[{id:'theme-identity',uid:'theme-uid',name:'主题测试身份',is_default:true}]},'/api/rtc':{ice_servers:[]}}:{})}});
  await page.route('**/api/preferences',route=>{if(route.request().method()==='PATCH'){const body=route.request().postDataJSON();writes.push(body);preferences={...preferences,...body.patch};}return route.fulfill({json:{account:1,initialized:true,preferences}});});
  await page.route('**/api/admin/home',route=>{if(route.request().method()==='POST')home=route.request().postDataJSON().home;return route.fulfill({json:home});});
  await page.route('**/api/client-build',route=>route.fulfill({json:{bundle:null,season:home.season,version:'1.0.8'}}));
  return {writes,publish:season=>home={...home,season}};
}
async function appearance(page){await page.getByRole('button',{name:'账号与个人资料'}).click();await page.getByRole('button',{name:'外观 · 社区主题'}).click();return page.getByRole('dialog',{name:'外观与社区主题'});}

test('administrator previews all internal palettes locally, then publishes with the existing password form',async({page})=>{
  await fixture(page,{admin:true});await page.goto('/');await expect(page.locator('html')).toHaveAttribute('data-season','winter');
  await page.getByRole('button',{name:'站点管理',exact:true}).click();await page.getByRole('tab',{name:'外观、协议与公告'}).click();
  const editor=page.locator('.season-editor');await expect(editor.getByRole('button')).toHaveCount(6);
  for(const theme of ['dark','light']){
    await page.evaluate(value=>document.documentElement.dataset.theme=value,theme);
    for(const name of ['经典','新春 · 灯火','中秋 · 月泊','圣诞 · 冬信','万圣 · 夜游','跨年 · 星序']){
      await editor.getByRole('button',{name,exact:true}).click();await expect(page.locator('html')).toHaveAttribute('data-season','winter');
      await expect(editor.locator('.season-preview-scene svg')).toHaveCount(name==='经典'?0:1);
      const ratio=await editor.locator('.season-preview-action').evaluate(n=>{
        const s=getComputedStyle(n),rgb=c=>c.match(/[\d.]+/g).slice(0,3).map(Number),lum=c=>rgb(c).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;}).reduce((v,x,i)=>v+x*[.2126,.7152,.0722][i],0),a=lum(s.color),b=lum(s.backgroundColor);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);
      });expect(ratio).toBeGreaterThanOrEqual(4.5);
      if(theme==='dark'&&name!=='经典')await editor.locator('.season-preview').screenshot({path:test.info().outputPath(`objects-preview-${await editor.locator('.season-preview').getAttribute('data-season')}.png`)});
    }
  }
  await editor.getByRole('button',{name:'新春 · 灯火',exact:true}).click();
  for(const width of [320,390,1280]){await page.setViewportSize({width,height:900});expect(await editor.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);}
  await editor.screenshot({path:test.info().outputPath('season-admin-preview.png')});
  await page.getByLabel('验证当前管理员密码').fill('synthetic admin password');await page.getByRole('button',{name:'保存并发布首页'}).click();
  await expect(page.locator('html')).toHaveAttribute('data-season','spring');
});

test('festival objects are scoped, noninteractive, reduced-motion safe and preserve the site image on small screens',async({page})=>{
  const image='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=';
  let home={...base,image};const external=[];
  page.on('request',r=>{if(new URL(r.url()).hostname!=='127.0.0.1')external.push(r.url());});
  await appFixture(page,{replies:{'/api/site':home,'/api/preferences':{account:1,initialized:true,preferences:{seasonPreference:'follow'}}}});
  await page.route('**/api/client-build',route=>route.fulfill({json:{bundle:null,season:home.season}}));
  const initialBuild=page.waitForResponse('**/api/client-build');
  await page.goto('/');await (await initialBuild).finished();await page.waitForLoadState('networkidle');await page.emulateMedia({reducedMotion:'reduce'});
  for(const id of ['spring','midautumn','winter','halloween','newyear']){
    home={...home,season:id};await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));
    const art=page.locator('.community-now .season-art');await expect(art).toHaveAttribute('data-festival',id);
    await expect(page.locator('.community-image-card img')).toHaveAttribute('src',image);
    expect(await art.locator('path').count()).toBeGreaterThan(8);
    expect(await art.evaluate(n=>getComputedStyle(n).pointerEvents)).toBe('none');
    expect(await art.locator('.season-art-spark').evaluateAll(nodes=>nodes.every(n=>getComputedStyle(n).animationName==='none'))).toBe(true);
    expect(await page.locator('.season-art [id]').evaluateAll(nodes=>new Set(nodes.map(n=>n.id)).size===nodes.length)).toBe(true);
    expect(await page.locator('.season-art').evaluateAll(nodes=>nodes.every(n=>!n.querySelector('image,foreignObject,script,a,[href]')))).toBe(true);
    const width=test.info().project.name.includes('mobile')?320:1280;await page.setViewportSize({width,height:900});
    expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
    const box=await art.boundingBox();expect(box.width).toBeGreaterThan(80);expect(box.height).toBeGreaterThan(70);
    await page.locator('.community-welcome').screenshot({path:test.info().outputPath(`festival-${id}.png`)});
  }
  expect(external).toEqual([]);
  home={...home,season:'classic'};await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await expect(page.locator('.season-art')).toHaveCount(0);
});

test('public festival homepage shows the full scene without blocking its login action',async({page})=>{
  await appFixture(page,{loggedIn:false,replies:{'/api/site':{...base,season:'midautumn'}}});
  await page.goto('/');const art=page.locator('.site-hero .hero-art>.season-art');
  await expect(art).toHaveAttribute('data-festival','midautumn');
  await expect(page.getByRole('button',{name:'登录，开始相聚'})).toBeVisible();
  if(test.info().project.name.includes('mobile'))await page.setViewportSize({width:320,height:844});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)).toBeLessThanOrEqual(1);
  await page.locator('.site-hero').screenshot({path:test.info().outputPath('festival-public.png')});
  await page.getByRole('button',{name:'登录，开始相聚'}).click();await expect(page.locator('.auth-page')).toBeVisible();
});

test('all festivals animate and real button clicks keep bounded effects that expire',async({page})=>{
  const f=await fixture(page);await page.emulateMedia({reducedMotion:'no-preference'});await page.goto('/');await page.waitForLoadState('networkidle');
  const moving={spring:'.season-art-main',midautumn:'.season-art-clouds>path',winter:'.season-art-snow>path',halloween:'.season-art-bats>path',newyear:'.season-art-second-hand'};
  for(const[id,selector]of Object.entries(moving)){
    f.publish(id);await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));
    await expect(page.locator('.season-home-art')).toHaveAttribute('data-festival',id);
    const part=page.locator('.season-home-art').locator(selector).first(),before=await part.evaluate(n=>getComputedStyle(n).transform);
    await expect.poll(()=>part.evaluate(n=>getComputedStyle(n).transform)).not.toBe(before);
    const layer=page.locator('.season-click-layer');await expect(layer).toHaveAttribute('data-festival',id);
    await page.getByRole('button',{name:'首页',exact:true}).first().click();await expect(layer.locator('.season-click-burst')).toHaveCount(1);
    expect(await layer.evaluate(n=>getComputedStyle(n).pointerEvents)).toBe('none');
    await expect(layer.locator('.season-click-burst')).toHaveCount(0,{timeout:2200});
  }
  const button=page.getByRole('button',{name:'首页',exact:true}).first();
  for(let i=0;i<7;i++){await button.click();expect(await page.locator('.season-click-burst').count()).toBeLessThanOrEqual(test.info().project.name.includes('mobile')?2:3);await page.waitForTimeout(130);}
  await page.emulateMedia({reducedMotion:'reduce'});await expect(page.locator('.season-click-burst')).toHaveCount(0);await button.click();await expect(page.locator('.season-click-burst')).toHaveCount(0);
  expect(await page.locator('.season-art *').evaluateAll(nodes=>nodes.every(n=>getComputedStyle(n).animationName==='none'))).toBe(true);
});

test('drag, scroll, inputs and right clicks do not generate effects; hidden pages pause and classic removes listeners',async({page})=>{
  const f=await fixture(page);await page.emulateMedia({reducedMotion:'no-preference'});await page.goto('/');await page.waitForLoadState('networkidle');
  async function gesture(kind){await page.evaluate(kind=>{
    const target=kind==='input'?Object.assign(document.createElement('input'),{id:'effect-input'}):document.querySelector('.community-welcome');
    if(kind==='input')document.body.append(target);
    const init={bubbles:true,isPrimary:true,pointerId:5,button:kind==='right'?2:0,clientX:100,clientY:200};
    target.dispatchEvent(new PointerEvent('pointerdown',init));
    if(kind==='scroll')document.dispatchEvent(new Event('scroll'));
    if(kind==='multitouch')target.dispatchEvent(new PointerEvent('pointerdown',{...init,isPrimary:false,pointerId:6}));
    target.dispatchEvent(new PointerEvent('pointerup',{...init,clientX:kind==='drag'?160:100}));
    if(kind==='input')target.remove();
  },kind);}
  for(const kind of ['drag','scroll','input','right','multitouch']){await gesture(kind);await expect(page.locator('.season-click-burst')).toHaveCount(0);}
  await page.getByRole('button',{name:'首页',exact:true}).first().click();await expect(page.locator('.season-click-burst')).toHaveCount(1);
  await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,get:()=>true});document.dispatchEvent(new Event('visibilitychange'));});
  await expect(page.locator('html')).toHaveAttribute('data-season-paused','');await expect(page.locator('.season-click-burst')).toHaveCount(0);
  expect(await page.locator('.season-art .season-art-main').first().evaluate(n=>getComputedStyle(n).animationPlayState)).toBe('paused');
  f.publish('classic');
  await page.evaluate(()=>{delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));});
  await expect(page.locator('html')).not.toHaveAttribute('data-season-paused','');
  await expect(page.locator('.season-click-layer')).toHaveCount(0);await expect(page.locator('.season-art')).toHaveCount(0);
  await page.getByRole('button',{name:'首页',exact:true}).first().click();await expect(page.locator('.season-click-burst')).toHaveCount(0);
});

test('classic preference persists with the account and later site broadcasts cannot override it',async({page})=>{
  const f=await fixture(page);await page.goto('/');await expect(page.locator('html')).toHaveAttribute('data-season','winter');
  let panel=await appearance(page);await panel.getByRole('button',{name:'始终使用经典'}).click();await expect(page.locator('html')).toHaveAttribute('data-season','classic');
  await expect.poll(()=>f.writes.some(w=>w.account===1&&w.patch.seasonPreference==='classic')).toBe(true);
  await page.reload();await page.waitForLoadState('networkidle');await expect(page.locator('html')).toHaveAttribute('data-season','classic');
  const broadcast=page.waitForResponse('**/api/client-build');
  f.publish('halloween');await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await (await broadcast).finished();await page.waitForLoadState('networkidle');await expect(page.locator('html')).toHaveAttribute('data-season','classic');
  panel=await appearance(page);await panel.getByRole('button',{name:'跟随站点主题'}).click();await expect(page.locator('html')).toHaveAttribute('data-season','halloween');
  await panel.getByRole('button',{name:'浅色',exact:true}).click();await expect(page.locator('html')).toHaveAttribute('data-theme','light');
  await panel.screenshot({path:test.info().outputPath('season-user-preferences.png')});
  f.publish('https://evil.example/theme.css');await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await expect(page.locator('html')).toHaveAttribute('data-season','classic');
});

test('season changes preserve live voice transport and a mounted watch player',async({page})=>{
  await page.goto('/');if(!await page.evaluate(()=>typeof AudioContext!=='undefined'))test.skip(true,'This local WebKit build lacks AudioContext; actual voice compatibility remains unverified');
  await page.addInitScript(()=>{window.__rtcCreations=0;window.__audioCreations=0;for(const[name,counter]of[['RTCPeerConnection','__rtcCreations'],['AudioContext','__audioCreations']]){const Original=window[name];if(Original)window[name]=new Proxy(Original,{construct(target,args,newTarget){window[counter]++;return Reflect.construct(target,args,newTarget);}});}});
  const f=await fixture(page,{watch:true,voice:true});let joins=0,loads=0;
  await page.routeWebSocket('**/api/connect',socket=>socket.onMessage(text=>{const value=JSON.parse(String(text));if(value.identity){joins++;socket.send(JSON.stringify({type:'state',server:'Theme test',own:1,canSpeak:true,channels:[{id:1,parent:0,order:0,name:'主题测试频道',password:false}],members:[{id:1,channel:1,name:'主题测试用户',uid:'theme-uid',avatarHash:'',muted:false,deafened:false,description:'',talkPower:0,serverGroups:[],channelGroup:0}]}));}}));
  await page.route('https://www.bilibili.com/blackboard/live/**',route=>{loads++;return route.fulfill({contentType:'text/html',body:'<!doctype html><p>隔离播放器，无真实视频</p>'});});
  await page.goto('/app');await page.getByRole('button',{name:'连接服务器',exact:true}).first().click();await page.getByRole('checkbox',{name:'仅收听，不打开麦克风'}).check();await page.getByRole('button',{name:'连接',exact:true}).click();
  await expect.poll(()=>joins,{timeout:10000}).toBe(1);await expect(page.getByRole('button',{name:/^断开(连接|服务器)$/}).first()).toBeEnabled();
  const counts=await page.evaluate(()=>[window.__rtcCreations,window.__audioCreations]);
  await page.getByRole('button',{name:'联合观赛',exact:true}).click();await page.getByRole('button',{name:'加载官方播放器'}).click();const frame=page.locator('iframe');await expect(frame).toHaveCount(1);await frame.evaluate(n=>n.dataset.continuity='preserved');
  const lowResource=page.getByRole('switch',{name:'低占用模式'});await lowResource.click();await expect(lowResource).toHaveAttribute('aria-checked','true');await lowResource.click();await expect(lowResource).toHaveAttribute('aria-checked','false');
  expect(joins).toBe(1);expect(await page.evaluate(()=>[window.__rtcCreations,window.__audioCreations])).toEqual(counts);
  const panel=await appearance(page);await panel.getByRole('button',{name:'始终使用经典'}).click();await panel.getByRole('button',{name:'浅色',exact:true}).click();await panel.getByRole('button',{name:'关闭',exact:true}).click();
  f.publish('spring');await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));
  await expect(frame).toHaveAttribute('data-continuity','preserved');expect(loads).toBe(1);expect(joins).toBe(1);expect(await page.evaluate(()=>[window.__rtcCreations,window.__audioCreations])).toEqual(counts);
  await page.getByRole('button',{name:'语音空间',exact:true}).click();await expect(page.getByRole('button',{name:/^断开(连接|服务器)$/}).first()).toBeEnabled();
});
