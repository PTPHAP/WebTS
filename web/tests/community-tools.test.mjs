import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
import * as React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url),options={module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX};
async function load(file,extra={}){const context=vm.createContext({exports:{},URL,URLSearchParams,TextEncoder,require,...extra});vm.runInContext(ts.transpileModule(await readFile(new URL(`../src/${file}`,import.meta.url),'utf8'),{compilerOptions:options}).outputText,context);return context.exports;}
const text=await load('ts-text.ts');
const favorites=await load('favorites.ts'),diagnostics=await load('voice-diagnostics.ts');
const conversation=await load('TsConversation.tsx',{require:n=>n==='./ts-text'?text:n==='./Stickers'?{StickerPicker:()=>null}:n==='./chat'?{chatVisible:(m,s)=>m.scope===s}:require(n)});
test('channel invites contain no secrets and accept only bounded configured identifiers',()=>{
 const value={server:'fixture',channel:7,uid:'server/uid=',name:'房间',password:'must never share'};const link=favorites.invitation(value,'https://webts.example');assert.ok(!link.includes('password'));assert.equal(favorites.readInvitation(new URL(link).search).channel,7);
 for(const search of ['?server=../../admin&channel=1','?server=fixture&channel=Infinity','?server=fixture&channel=-1','?server=fixture&channel=9007199254740992','?server=fixture&channel=1&server_uid=https://secret'])assert.equal(favorites.readInvitation(search),undefined);
 assert.equal(favorites.readFavorites(Array.from({length:20},()=>value)).length,8);
 assert.ok(!JSON.stringify(favorites.readFavorites([value])).includes('password'));
});
const input={connected:true,reconnecting:false,transport:'connected',capture:'running',listenOnly:false,muted:false,deafened:false,away:false,canSpeak:true,playbackBlocked:false,otherSpeaking:false,quality:{rtt:20,received:50,sent:40,route:'direct'},cipher:'AES-256-GCM',gain:1,volume:1};
test('diagnosis distinguishes intentional mute, permission denial and browser playback; exports strict whitelist',()=>{
 assert.equal(diagnostics.voiceDiagnosis(input)[0].title,'未发现明显的设置或连接异常');
 for(const [change,title] of [[{muted:true},'麦克风主动暂停'],[{canSpeak:false},'当前身份没有发言权限'],[{playbackBlocked:true},'浏览器尚未允许播放'],[{otherSpeaking:true,quality:{received:0}},'有人发言，但尚未收到音频包']])assert.ok(diagnostics.voiceDiagnosis({...input,...change}).some(v=>v.title===title));
 const report=diagnostics.diagnosticReport({...input,capture:'secret@example.com',cipher:'secret key',quality:{rtt:NaN,address:'192.168.1.1',token:'secret',route:'not-a-route'},password:'secret',name:'private'});const json=JSON.stringify(report);for(const word of ['secret','example.com','192.168','private','password'])assert.ok(!json.includes(word));assert.equal(report.metrics.rttMs,null);
});
test('mentions require complete name boundaries and quoted replies are plain bounded text',()=>{
 assert.equal(conversation.mentioned('hello @Bob!', 'Bob'),true);assert.equal(conversation.mentioned('@Bobby', 'Bob'),false);assert.equal(conversation.mentioned('email@Bob', 'Bob'),false);assert.equal(conversation.mentioned('[b]@妮可[/b]，你好','妮可'),true);
 const reply=conversation.replyText({name:'peer',text:'[b]quoted[/b]'},'answer');assert.equal(reply,'↪ peer: quoted\nanswer');assert.ok(!reply.includes('[b]'));
});
test('event reminders exclude cancelled, unregistered, expired and already announced events',async()=>{
 const events=await load('CommunityEvents.tsx',{require:n=>n==='./api'?{api:()=>Promise.reject(Error('no network'))}:require(n)});
 const one={id:'one',starts:100,ends:300,attending:true,cancelled:false};const list=[one,{...one,id:'two',attending:false},{...one,id:'three',cancelled:true},{...one,id:'old',starts:1},{...one,id:'ended',ends:99}];assert.equal(events.eventReminders(list,100,new Set()).length,1);assert.equal(events.eventReminders(list,100,new Set(['one:100'])).length,0);
});

test('searching and hidden tabs do not acknowledge incoming TS messages as read',async()=>{
 for(const [search,hidden] of [['older message',false],['',true]]){
  const effects=[],listeners=new Map();let stateIndex=0,refIndex=0,unread=[];
  const module=await load('TsConversation.tsx',{document:{hidden,addEventListener:(k,f)=>listeners.set(k,f),removeEventListener:k=>listeners.delete(k)},require:n=>n==='react'?{...React,useState:()=>{const index=stateIndex++;return [index===0?search:index===2?unread:undefined,v=>{if(index===2)unread=typeof v==='function'?v(unread):v;}];},useRef:v=>({current:refIndex++===1?new Set([1]):v}),useEffect:f=>effects.push(f)}:n==='./ts-text'?text:n==='./Stickers'?{StickerPicker:()=>null}:n==='./chat'?{chatVisible:(m,s)=>m.scope===s}:require(n)});
  module.TsConversation({chat:[{key:1,scope:'channel',from:2,text:'older'},{key:2,scope:'channel',from:2,text:'new'}],scope:'channel',nickname:'me',own:1,message:'',setScope(){},setMessage(){},send(){},render(){},channel:'fixture',privateCount:0,onPrivate(){}});
  effects[1]();assert.deepEqual(Array.from(unread),[2]);effects[2]();listeners.get('visibilitychange')();assert.deepEqual(Array.from(unread),[2]);
 }
});
test('community cards safely render malicious text and retain native voice footer outside the scroll region',async()=>{
 const row={id:'one',title:'<script>bad</script>',description:'<img src=x onerror=bad()>',starts:100,ends:300,community:false,cancelled:false,attending:false,participants:1,server:'fixture',channel:7};let index=0;
 const events=await load('CommunityEvents.tsx',{crypto:{randomUUID:()=> 'id'},require:n=>n==='react'?{...React,useState:initial=>[index++===0?[row]:initial,()=>{}],useEffect(){},useCallback:f=>f,useRef:v=>({current:v})}:n==='./api'?{}:require(n)});
 const html=renderToStaticMarkup(events.CommunityEvents({admin:false,servers:[{id:'fixture',name:'fixture'}],loggedIn:true,onJoin(){},onLogin(){},voiceBar:React.createElement('footer',{},'voice')}));assert.ok(html.includes('&lt;script&gt;'));assert.ok(!html.includes('<script>'));assert.ok(html.endsWith('</main><footer>voice</footer>'));
});

test('update polling is single-flight, aborts on unmount and never reloads an active call',async()=>{
 const effects=[],timers=new Map(),listeners=new Map(),states=[];let finish,calls=0,reloads=0,signal,next=0;
 const document={hidden:false,querySelector:()=>({src:'https://fixture.example/assets/index-old.js'}),addEventListener:(k,f)=>listeners.set(k,f),removeEventListener:k=>listeners.delete(k)};
 const module=await load('UpdateNotice.tsx',{document,location:{href:'https://fixture.example',reload:()=>reloads++},AbortController,confirm:()=>true,setInterval:f=>{timers.set(++next,f);return next;},clearInterval:k=>timers.delete(k),setTimeout:()=>99,clearTimeout(){},fetch:(_,options)=>{calls++;signal=options.signal;return new Promise(r=>finish=r);},require:n=>n==='react'?{...React,useState:v=>{const s={value:v};states.push(s);return [v,v=>s.value=v];},useEffect:f=>effects.push(f())}:require(n)});
 module.UpdateNotice({inCall:true});assert.equal(calls,1);timers.values().next().value();assert.equal(calls,1);effects[0]();assert.equal(signal.aborted,true);finish({ok:true,json:async()=>({bundle:'/assets/index-new.js'})});for(let n=0;n<10;n++)await Promise.resolve();assert.equal(states[0].value,'');assert.equal(timers.size,0);assert.equal(listeners.size,0);assert.equal(reloads,0);
 let index=0;const rendering=await load('UpdateNotice.tsx',{confirm:()=>true,location:{reload:()=>reloads++},require:n=>n==='react'?{...React,useState:()=>[index++===0?'/assets/index-new.js':'',()=>{}],useEffect(){}}:require(n)});
 const html=renderToStaticMarkup(rendering.UpdateNotice({inCall:true}));assert.match(html,/button[^>]*disabled=""[^>]*>刷新/);assert.equal(reloads,0);
 for(const bad of ['/assets/index-a.js?secret=1','https://evil.example/assets/index-a.js','/assets/index-../../a.js'])assert.equal(module.bundlePath(bad),undefined);
});

test('activity reminders pause in background, avoid overlapping requests and suppress stale-account callbacks',async()=>{
 const effects=[],timers=new Map(),listeners=new Map(),notifications=[];let finish,calls=0;
 const document={hidden:false,addEventListener:(k,f)=>listeners.set(k,f),removeEventListener:k=>listeners.delete(k)};
 const module=await load('CommunityEvents.tsx',{document,AbortSignal,setInterval:f=>{timers.set(1,f);return 1;},clearInterval:k=>timers.delete(k),require:n=>n==='react'?{...React,useRef:v=>({current:v}),useEffect:f=>effects.push(f())}:n==='./api'?{api:()=>{calls++;return new Promise(r=>finish=r);}}:require(n)});
 module.CommunityAlerts({onNotify:(...v)=>notifications.push(v)});assert.equal(calls,1);timers.get(1)();assert.equal(calls,1);finish({items:[],server_time:100});for(let n=0;n<10;n++)await Promise.resolve();document.hidden=true;timers.get(1)();assert.equal(calls,1);document.hidden=false;listeners.get('visibilitychange')();finish({items:[{id:'one',starts:100,ends:300,mine:false,attending:true,cancelled:false,title:'fixture'}],server_time:100});for(let n=0;n<10;n++)await Promise.resolve();assert.equal(notifications.length,2);timers.get(1)();effects[0]();finish({items:[{id:'two',starts:100,ends:300,mine:false,attending:true,cancelled:false,title:'late'}],server_time:100});for(let n=0;n<10;n++)await Promise.resolve();assert.equal(notifications.length,2);assert.equal(timers.size,0);assert.equal(listeners.size,0);
});
