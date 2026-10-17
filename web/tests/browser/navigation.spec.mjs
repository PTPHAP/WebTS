import {test,expect} from '@playwright/test';

import {appFixture as fixture} from './app-fixture.mjs';

test('repeated navigation does not leave activity pages or duplicate voice bars in other views',async({page})=>{
  await fixture(page);
  const errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  page.on('console',message=>{if(message.type()==='error')errors.push(message.text());});
  await page.goto('/app');
  await expect(page.getByRole('button',{name:'账号与个人资料'})).toBeVisible();
  for(let cycle=0;cycle<12;cycle++){
    await page.getByRole('button',{name:'联合观赛',exact:true}).click();
    await expect(page.locator('.app')).toHaveAttribute('data-view','watch');
    await page.getByRole('button',{name:'社区',exact:true}).click();await expect(page.locator('.app')).toHaveAttribute('data-view','community');await expect(page.locator('.community-hub')).toHaveCount(1);await expect(page.locator('.workspace-shell')).toHaveCount(0);
    await page.getByRole('button',{name:'首页',exact:true}).click();
    await expect(page.locator('.app')).toHaveAttribute('data-view','home');
    await expect(page.getByRole('button',{name:'社区活动',exact:true})).toHaveCount(0);
    await page.getByRole('button',{name:'语音空间',exact:true}).click();
    await expect(page.locator('.app')).toHaveAttribute('data-view','app');
    await expect(page.getByRole('button',{name:'社区活动',exact:true})).toHaveCount(0);
    await expect(page.locator('.workspace-shell')).toHaveCount(1);
  }
  expect(errors.filter(error=>!error.includes('status of 404'))).toEqual([]);
});

test('a playing iframe keeps its instance while navigating through activities and voice space',async({page})=>{
  await fixture(page,{watch:true});
  let loads=0;
  await page.route('https://www.bilibili.com/blackboard/live/**',route=>{
    loads++;
    return route.fulfill({contentType:'text/html',body:'<!doctype html><title>隔离播放器</title><p>没有实际直播或外部网络请求</p>'});
  });
  await page.goto('/watch');
  await page.getByRole('button',{name:'加载官方播放器'}).click();
  const player=page.locator('iframe');
  await expect(player).toHaveCount(1);
  await expect.poll(()=>loads).toBe(1);
  await player.evaluate(node=>node.dataset.continuity='original');
  for(let cycle=0;cycle<6;cycle++){
    await page.getByRole('button',{name:'首页',exact:true}).click();
    await expect(page.getByRole('button',{name:'社区活动',exact:true})).toHaveCount(0);
    await page.getByRole('button',{name:'语音空间',exact:true}).click();
    await expect(page.locator('.workspace-shell')).toHaveCount(1);
    await page.getByRole('button',{name:'联合观赛',exact:true}).click();
    await page.getByRole('button',{name:'社区',exact:true}).click();await page.getByRole('button',{name:'联合观赛',exact:true}).click();
    await expect(player).toHaveCount(1);
    await expect(player).toHaveAttribute('data-continuity','original');
    expect(loads).toBe(1);
  }
});


test('WebTS nickname marker is compact in the channel tree and member profile',async({page})=>{
  test.skip(!(await page.evaluate(()=>typeof AudioContext==='function')), 'This browser build lacks AudioContext; it cannot validate a connected voice session.');
  let connections=0;
  await fixture(page,{replies:{
    '/api/identities':{identities:[{id:'fixture-identity',name:'测试身份',uid:'fixture-uid',is_default:true}]},
    '/api/rtc':{ice_servers:[]},
  }});
  await page.routeWebSocket('**/api/connect',socket=>{connections++;socket.onMessage(text=>{
    const request=JSON.parse(String(text));
    if(request.identity)socket.send(JSON.stringify({type:'state',server:'本地服务器',own:1,canSpeak:true,channels:[{id:1,parent:0,order:0,name:'公共大厅',password:false}],members:[{id:1,channel:1,name:'测试用户',webts:true,uid:'fixture-uid',avatarHash:'',muted:false,deafened:false,description:'',talkPower:0,serverGroups:[],channelGroup:0},{id:2,channel:1,name:'普通客户端',uid:'native-uid',avatarHash:'',muted:false,deafened:false,description:'',talkPower:0,serverGroups:[],channelGroup:0}]}));
  });});
  await page.goto('/app');
  await expect(page.getByRole('button',{name:'连接服务器',exact:true}).first()).toBeEnabled();
  await page.getByRole('button',{name:'连接服务器',exact:true}).first().click();
  await page.getByRole('checkbox',{name:'仅收听，不打开麦克风'}).check();
  await page.getByRole('button',{name:'连接',exact:true}).click();
  if(test.info().project.name.includes('mobile'))await page.getByRole('navigation',{name:'手机主导航'}).getByRole('button',{name:'频道',exact:true}).click();
  const member=page.locator('.member-row').filter({hasText:'测试用户'});
  await expect(member.locator('.webts-client-tag')).toHaveText('WebTS');
  await expect(page.locator('.member-row').filter({hasText:'普通客户端'}).locator('.webts-client-tag')).toHaveCount(0);
  await member.click();
  await expect(page.locator('.profile h2 .webts-client-tag')).toHaveText('WebTS');
  await expect(page.locator('.profile h2')).toContainText('测试用户');
  await page.locator('.profile').screenshot({path:test.info().outputPath('webts-tag.png')});
  await expect(page.getByRole('button',{name:/^断开(连接|服务器)$/}).first()).toBeVisible();
  for(let cycle=0;cycle<3;cycle++){await page.getByRole('link',{name:'WebTS 首页',exact:true}).click();await page.getByRole('button',{name:'社区',exact:true}).click();await expect(page.locator('.community-hub')).toHaveCount(1);await expect(page.locator('.workspace-shell')).toHaveCount(0);await page.getByRole('button',{name:'语音空间',exact:true}).click();if(test.info().project.name.includes('mobile'))await page.getByRole('navigation',{name:'手机主导航'}).getByRole('button',{name:'频道',exact:true}).click();await expect(page.locator('.member-row').filter({hasText:'测试用户'})).toBeVisible();await expect(page.locator('.voice-bar:visible')).toHaveCount(1);await expect(page.locator('.voice-bar')).toHaveCount(2);}
  expect(connections).toBe(1);
});
