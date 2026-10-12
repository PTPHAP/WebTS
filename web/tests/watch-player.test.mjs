import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import * as React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import ts from 'typescript';

const require=createRequire(import.meta.url),options={module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX};
async function helper(file){const context=vm.createContext({exports:{},URL});vm.runInContext(ts.transpileModule(await readFile(new URL(`../src/${file}`,import.meta.url),'utf8'),{compilerOptions:options}).outputText,context);return context.exports;}
const watch=await helper('watch.ts'),geometry=await helper('watch-window.ts');
const code=ts.transpileModule(await readFile(new URL('../src/WatchPage.tsx',import.meta.url),'utf8'),{compilerOptions:options}).outputText;
const config={...watch.emptyWatch,enabled:true,embed_enabled:true,url:'https://live.bilibili.com/123',title:'Example <script>alert(1)</script>',score_mode:'manual'};
const event={enabled:true,event_id:'fixture',config,platform:'bilibili',server_time:100,score:null};
function render(active,loaded=true,e=event,stage={x:20,y:140,width:900,height:506},props={}){
 let index=0;const overrides={0:e,2:loaded?`fixture:${watch.playerUrl(e.config??config,'example.com')}`:'',13:{left:0,top:72,right:1280,bottom:640},14:stage};
 const context=vm.createContext({exports:{},innerWidth:1280,innerHeight:720,location:{hostname:'example.com'},require:name=>name==='react'?{...React,useState:initial=>[Object.hasOwn(overrides,index)?overrides[index++]:((index++),initial),()=>{}],useRef:value=>({current:value}),useEffect(){},useLayoutEffect(){}}:name==='./watch'?watch:name==='./watch-window'?geometry:name==='./Stickers'?{StickerPicker:()=>null}:name==='./api'?{api:()=>{throw Error('render check must not send requests');}}:name.endsWith('.css')?{}:require(name)});
 vm.runInContext(code,context);
 const element=context.exports.WatchPage({active,voiceBar:React.createElement('footer',{},'voice'),connected:false,members:[],avatars:{},speaking:{},channel:'',chat:null,onSend:()=>false,onJoin(){},onLogin(){},onVoice(){},onExpand(){},...props});
 const nodes=[];
 function walk(value,path=[]){if(Array.isArray(value))value.forEach((v,i)=>walk(v,[...path,i]));else if(React.isValidElement(value)){nodes.push({element:value,path});walk(value.props.children,[...path,'children']);}}
 walk(element);return {element,nodes};
}
test('main and mini use the same iframe reconciliation path, source, key and security attributes',()=>{
 const full=render(true),mini=render(false);
 const a=full.nodes.find(n=>n.element.type==='iframe'),b=mini.nodes.find(n=>n.element.type==='iframe');
 assert.deepEqual(a.path,b.path);assert.equal(a.element.key,b.element.key);assert.equal(a.element.props.src,b.element.props.src);
 assert.equal(full.nodes.find(n=>n.element.type==='main').element.props.hidden,false);assert.equal(mini.nodes.find(n=>n.element.type==='main').element.props.hidden,true);
 assert.equal(mini.nodes.find(n=>n.element.props.className==='watch-floating').element.props['data-mini'],true);
 assert.equal(b.element.props.sandbox,'allow-scripts allow-same-origin allow-forms allow-presentation allow-popups');
 assert.ok(!b.element.props.allow.includes('microphone'));assert.ok(!b.element.props.sandbox.includes('top-navigation'));
});

test('watch room renders the complete validated avatar URL for members and music bots',()=>{
 const image='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==';
 const members=[{id:1,name:'频道成员',uid:'person',avatarHash:'one'},{id:2,name:'音乐机器人',uid:'bot',avatarHash:'two'},{id:3,name:'无头像成员',uid:'none',avatarHash:''}];
 const view=render(true,true,event,undefined,{connected:true,members,avatars:{'person:one':image,'bot:two':image}});
 const images=view.nodes.filter(n=>n.element.type==='img');
 assert.equal(images.length,2);
 for(const item of images)assert.equal(item.element.props.src,image);
 const html=renderToStaticMarkup(view.element);assert.ok(!html.includes('base64,data:image'));assert.ok(html.includes('无头像成员'));
});

test('Twitch stage respects the official 400 by 300 minimum instead of squeezing the player',()=>{
 const twitch={...event,config:{...config,url:'https://www.twitch.tv/example'},platform:'twitch'};
 const full=render(true,true,twitch);
 assert.equal(full.nodes.find(n=>n.element.props.className==='watch-player-slot').element.props.style.minHeight,300);
 assert.equal(full.nodes.filter(n=>n.element.type==='iframe').length,1);
 const narrow=render(true,true,twitch,{x:12,y:100,width:390,height:300});
 assert.equal(narrow.nodes.filter(n=>n.element.type==='iframe').length,0);
});
test('a closed, not-yet-loaded or revoked player cannot remain in the background',()=>{
 for(const state of [render(false,false),render(false,true,{enabled:false}),render(true,false)])assert.equal(state.nodes.filter(n=>n.element.type==='iframe').length,0);
 const html=renderToStaticMarkup(render(true).element);assert.ok(html.includes('&lt;script&gt;'));assert.ok(!html.includes('<script>'));assert.ok(!html.includes('dangerouslySetInnerHTML'));
});

test('loaded hidden players still recheck access; unmount cancels timers and ignores pending responses',async()=>{
 for(const loaded of [false,true]){
  let index=0,calls=0,changes=0,resolve;
  const effects=[],timers=new Map(),intervals=new Map();let next=0;
  const overrides={0:event,2:loaded?`fixture:${watch.playerUrl(config,'example.com')}`:'',13:{left:0,top:72,right:1280,bottom:640},14:null};
  const context=vm.createContext({exports:{},innerWidth:1280,innerHeight:720,location:{hostname:'example.com'},document:{visibilityState:'hidden'},setTimeout:(fn,ms)=>{timers.set(++next,{fn,ms});return next;},clearTimeout:id=>timers.delete(id),setInterval:fn=>{intervals.set(++next,fn);return next;},clearInterval:id=>intervals.delete(id),require:name=>name==='react'?{...React,useState:initial=>[Object.hasOwn(overrides,index)?overrides[index++]:((index++),initial),()=>changes++],useRef:value=>({current:value}),useEffect:fn=>effects.push(fn),useLayoutEffect(){}}:name==='./watch'?watch:name==='./watch-window'?geometry:name==='./Stickers'?{StickerPicker:()=>null}:name==='./api'?{api:()=>{calls++;return new Promise(r=>resolve=r);}}:name.endsWith('.css')?{}:require(name)});
  vm.runInContext(code,context);
  context.exports.WatchPage({active:true,voiceBar:null,connected:false,members:[],avatars:{},speaking:{},channel:'',chat:null,onSend:()=>false,onJoin(){},onLogin(){},onVoice(){},onExpand(){}});
  const cleanup=effects[0]();
  assert.equal(calls,loaded?1:0);
  assert.equal(intervals.size,1);
  if(!loaded)assert.equal([...timers.values()][0].ms,5000);
  cleanup();
  if(resolve){resolve(event);await Promise.resolve();await Promise.resolve();}
  assert.equal(changes,0);assert.equal(timers.size,0);assert.equal(intervals.size,0);
 }
});
