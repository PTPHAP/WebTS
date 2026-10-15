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
      const ratio=await editor.locator('.season-preview-action').evaluate(n=>{
        const s=getComputedStyle(n),rgb=c=>c.match(/[\d.]+/g).slice(0,3).map(Number),lum=c=>rgb(c).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;}).reduce((v,x,i)=>v+x*[.2126,.7152,.0722][i],0),a=lum(s.color),b=lum(s.backgroundColor);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);
      });expect(ratio).toBeGreaterThanOrEqual(4.5);
    }
  }
  await editor.getByRole('button',{name:'新春 · 灯火',exact:true}).click();
  for(const width of [320,390,1280]){await page.setViewportSize({width,height:900});expect(await editor.evaluate(n=>n.scrollWidth-n.clientWidth)).toBeLessThanOrEqual(1);}
  await editor.screenshot({path:test.info().outputPath('season-admin-preview.png')});
  await page.getByLabel('验证当前管理员密码').fill('synthetic admin password');await page.getByRole('button',{name:'保存并发布首页'}).click();
  await expect(page.locator('html')).toHaveAttribute('data-season','spring');
});

test('classic preference persists with the account and later site broadcasts cannot override it',async({page})=>{
  const f=await fixture(page);await page.goto('/');await expect(page.locator('html')).toHaveAttribute('data-season','winter');
  let panel=await appearance(page);await panel.getByRole('button',{name:'始终使用经典'}).click();await expect(page.locator('html')).toHaveAttribute('data-season','classic');
  await expect.poll(()=>f.writes.some(w=>w.account===1&&w.patch.seasonPreference==='classic')).toBe(true);
  await page.reload();await expect(page.locator('html')).toHaveAttribute('data-season','classic');
  f.publish('halloween');await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await expect(page.locator('html')).toHaveAttribute('data-season','classic');
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
  await expect.poll(()=>joins).toBe(1);await expect(page.getByRole('button',{name:/^断开(连接|服务器)$/}).first()).toBeEnabled();
  const counts=await page.evaluate(()=>[window.__rtcCreations,window.__audioCreations]);
  await page.getByRole('button',{name:'联合观赛',exact:true}).click();await page.getByRole('button',{name:'加载官方播放器'}).click();const frame=page.locator('iframe');await expect(frame).toHaveCount(1);await frame.evaluate(n=>n.dataset.continuity='preserved');
  const panel=await appearance(page);await panel.getByRole('button',{name:'始终使用经典'}).click();await panel.getByRole('button',{name:'浅色',exact:true}).click();await panel.getByRole('button',{name:'关闭',exact:true}).click();
  f.publish('spring');await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));
  await expect(frame).toHaveAttribute('data-continuity','preserved');expect(loads).toBe(1);expect(joins).toBe(1);expect(await page.evaluate(()=>[window.__rtcCreations,window.__audioCreations])).toEqual(counts);
  await page.getByRole('button',{name:'语音空间',exact:true}).click();await expect(page.getByRole('button',{name:/^断开(连接|服务器)$/}).first()).toBeEnabled();
});
