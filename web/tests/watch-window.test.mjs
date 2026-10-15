import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';

const code=ts.transpileModule(await readFile(new URL('../src/watch-window.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const {miniWatchRect,watchShouldMount}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('mini player stays inside desktop, phone, keyboard and rotated visible bounds',()=>{
 for(const bounds of [{left:0,top:72,right:1360,bottom:908},{left:0,top:102,right:390,bottom:680},{left:0,top:102,right:320,bottom:565},{left:0,top:100,right:390,bottom:280},{left:40,top:160,right:340,bottom:420},{left:0,top:62,right:844,bottom:280}]){
  for(const point of [null,{x:-99999,y:-99999},{x:99999,y:99999}])for(const folded of [false,true]){
   const r=miniWatchRect(bounds,point,folded);
   assert.ok(r.x>=bounds.left&&r.y>=bounds.top);assert.ok(r.x+r.width<=bounds.right&&r.y+r.height<=bounds.bottom);
   assert.ok(r.width>=0&&r.height>=0);if(folded)assert.equal(r.collapsed,true);
  }
 }
 const desktop=miniWatchRect({left:0,top:72,right:1360,bottom:908},null,false);assert.equal(desktop.width,360);assert.equal(desktop.collapsed,false);
 const phone=miniWatchRect({left:0,top:102,right:390,bottom:680},null,false);assert.equal(phone.width,280);assert.equal(phone.collapsed,false);
 const keyboard=miniWatchRect({left:0,top:102,right:390,bottom:250},null,false);assert.equal(keyboard.collapsed,true);assert.equal(keyboard.height,44);
});
test('platform minimum player size is respected, with a collapsed fallback for small viewports',()=>{
 const minimum={width:400,height:300};
 const desktop=miniWatchRect({left:0,top:72,right:1360,bottom:908},null,false,minimum);assert.ok(desktop.width>=400&&desktop.height-92>=300);assert.equal(desktop.collapsed,false);
 const phone=miniWatchRect({left:0,top:102,right:390,bottom:680},null,false,minimum);assert.equal(phone.collapsed,true);assert.ok(phone.width<=374);
});
test('navigation retains the watch component only after it was visited; account key still isolates it',async()=>{
 assert.equal(watchShouldMount(false,'app'),false);assert.equal(watchShouldMount(false,'watch'),true);
 for(const view of ['app','home','privacy','terms','login'])assert.equal(watchShouldMount(true,view),true);
 const main=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8');
 assert.match(main,/watchShouldMount\(watchVisited,view\)&&<WatchPage key=\{`watch:\$\{user\?\.id\?\?'public'\}`\}/);
 assert.match(main,/active=\{view==='watch'\}/);
});
