import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/workspace-layout.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const {defaultLayout,readLayout,reorderPanels,resizePanels,phoneLayoutQuery,compactLayout,minimumWorkspaceWidth,readPanelOrder}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
const main=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8');
const phoneInitializer=main.match(/const\[phone\]=useState\((.*?)\);const scrollClickGuard/)[1];
function initialPhone(device,pointer=false){
  return vm.runInNewContext(`(${phoneInitializer})()`,{compactLayout,phoneLayoutQuery,matchMedia:()=>({matches:pointer}),navigator:device});
}

test('phone browsers remain compact when their pointer query reports desktop capabilities',()=>{
  for(const device of [
    {userAgent:'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)',maxTouchPoints:5},
    {userAgent:'Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 Chrome/140.0 Mobile Safari/537.36',maxTouchPoints:5},
    {userAgent:'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15) AppleWebKit/605.1.15',maxTouchPoints:5},
    {userAgent:'Mozilla/5.0',maxTouchPoints:0,userAgentData:{mobile:true}},
  ])assert.equal(initialPhone(device),true,device.userAgent);
});
test('desktop browsers and touch laptops retain columns regardless of window size',()=>{
  for(const device of [
    {userAgent:'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',maxTouchPoints:0,userAgentData:{mobile:false}},
    {userAgent:'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',maxTouchPoints:10},
    {userAgent:'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)',maxTouchPoints:0},
    {userAgent:'Mozilla/5.0 (X11; Linux x86_64)',maxTouchPoints:0},
  ])assert.equal(initialPhone(device),false,device.userAgent);
  assert.equal(initialPhone({userAgent:'Mozilla/5.0',maxTouchPoints:5},true),true);
});

test('phone layout ignores legacy mode overrides and mobile order is separate',()=>{
  for(const phone of [true,false]){
    assert.equal(compactLayout('columns',phone),phone);assert.equal(compactLayout('compact',phone),phone);
    assert.equal(compactLayout(null,phone),phone);
  }
  const mobile=readPanelOrder(['members','channels','chat']);assert.deepEqual(mobile,['members','channels','chat']);
  mobile.pop();assert.deepEqual(defaultLayout.order,['channels','chat','members']);
  for(const bad of [null,['members','members','chat'],['__proto__','chat','channels'],['chat','channels']])assert.deepEqual(readPanelOrder(bad),defaultLayout.order);
});
test('column layout has usable panel widths at every saved proportion without changing order',()=>{
  for(const layout of [defaultLayout,resizePanels(defaultLayout,0,-1000),resizePanels(defaultLayout,1,1000)]){
    const width=minimumWorkspaceWidth(layout);
    assert.ok(width*layout.widths.channels/100>=280);assert.ok(width*layout.widths.chat/100>=360);assert.ok(width*layout.widths.members/100>=240);
  }
});

test('narrow mouse-driven desktop windows do not become the phone layout',()=>{
  const matches=(width,hover,pointer)=>String(phoneLayoutQuery)
    .replace(/\(max-width:\s*(\d+)px\)/g,(_,limit)=>String(width<=Number(limit)))
    .replace(/\(hover:\s*(\w+)\)/g,(_,value)=>String(hover===value))
    .replace(/\(pointer:\s*(\w+)\)/g,(_,value)=>String(pointer===value))
    .split(/\s+and\s+/).every(value=>value==='true');
  for(const width of [320,600,700,760,900,1366])assert.equal(matches(width,'hover','fine'),false,`desktop ${width}`);
  assert.equal(matches(390,'none','coarse'),true);
  assert.equal(matches(760,'none','coarse'),true);
  assert.equal(matches(900,'none','coarse'),true);
});
test('saved layout accepts only three unique panels and bounded widths totaling 100',()=>{
  for(const value of [null,{}, {order:['chat','chat','channels'],widths:defaultLayout.widths},{order:defaultLayout.order,widths:{channels:Infinity,chat:54,members:24}},{order:defaultLayout.order,widths:{channels:22,chat:54,members:25}}])assert.deepEqual(readLayout(value),defaultLayout);
  assert.deepEqual(readLayout(reorderPanels(defaultLayout,'members','channels')).order,['members','channels','chat']);
});
test('moving and resizing preserve every panel, sum and minimum usable width',()=>{
  const moved=reorderPanels(defaultLayout,'channels','members');assert.deepEqual(moved.order,['chat','members','channels']);
  for(const layout of [defaultLayout,moved])for(const index of [0,1])for(const delta of [-1000,-2,2,1000]){
    const next=resizePanels(layout,index,delta);assert.deepEqual(readLayout(next),next);assert.equal(Object.values(next.widths).reduce((a,b)=>a+b),100);
  }
});
