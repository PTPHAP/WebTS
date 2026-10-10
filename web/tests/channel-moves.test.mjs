import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/channel-moves.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {ChannelMoves}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
function fixture(){const sent=[],asked=[],done=[],notices=[];const moves=new ChannelMoves(v=>sent.push(v),v=>asked.push(v),v=>done.push(v),v=>notices.push(v));return {moves,sent,asked,done,notices};}
test('password-protected channel first asks TS, privileged entry does not prompt',()=>{const f=fixture();f.moves.start({channel:2,name:'Locked'});assert.equal(f.sent[0].password,'');assert.equal(f.asked.length,0);f.moves.result({id:f.sent[0].id,ok:true});assert.equal(f.done.length,1);assert.equal(f.asked.length,0);});
test('only numeric channel-password error prompts; password retry is not retained',()=>{const f=fixture();f.moves.start({channel:2,name:'Locked',client:7});f.moves.result({id:f.sent[0].id,ok:false,code:781});assert.equal(f.asked.length,1);f.moves.password('fixture password');assert.equal(f.sent[1].password,'fixture password');assert.equal(f.sent[1].client,7);assert.notEqual(f.sent[0].id,f.sent[1].id);f.moves.result({id:f.sent[1].id,ok:true});f.moves.start({channel:2,name:'Locked'});assert.equal(f.sent[2].password,'');});
test('permission refusal, expired and other command results never request a password',()=>{const f=fixture();f.moves.start({channel:2,name:'Locked'});assert.equal(f.moves.result({id:'other',ok:false,code:781}),false);f.moves.result({id:f.sent[0].id,ok:false,code:2568,message:'permission denied'});assert.equal(f.asked.length,0);assert.equal(f.notices[0],'permission denied');f.moves.start({channel:3,name:'Other'});f.moves.result({id:f.sent[1].id,ok:false,message:'timeout'});assert.equal(f.asked.length,0);});
test('duplicate taps are bounded; cancel and disconnect discard stale results and password retry',()=>{const f=fixture();f.moves.start({channel:2,name:'Locked'});f.moves.start({channel:3,name:'Other'});assert.equal(f.sent.length,1);const old=f.sent[0].id;f.moves.reset();assert.equal(f.moves.result({id:old,ok:false,code:781}),false);f.moves.password('never sent');assert.equal(f.sent.length,1);f.moves.start({channel:3,name:'Other'});f.moves.result({id:f.sent[1].id,ok:false,code:781});f.moves.reset();f.moves.password('never sent');assert.equal(f.sent.length,2);});
test('remembering waits for TS confirmation, retries a cached password once, and evicts rejected cache',async()=>{
  const sent=[],asked=[],saved=[],forgot=[];const target={channel:2,name:'Room',context:'synthetic account / server'};
  const moves=new ChannelMoves(v=>sent.push(v),v=>asked.push(v),()=>{},()=>{},{load:async()=> 'remembered secret',save:async(t,p)=>saved.push([t,p]),forget:async t=>forgot.push(t)});
  moves.start(target);assert.equal(sent[0].password,'');moves.result({id:sent[0].id,ok:false,code:781});await new Promise(setImmediate);assert.equal(sent[1].password,'remembered secret');assert.equal(asked.length,0);
  moves.result({id:sent[1].id,ok:false,code:781});await new Promise(setImmediate);assert.equal(forgot.length,1);assert.equal(asked.length,1);
  moves.password('new secret',true);assert.equal(saved.length,0);moves.result({id:sent[2].id,ok:false,code:781});assert.equal(saved.length,0);moves.password('correct secret',true);moves.result({id:sent[3].id,ok:true});await new Promise(setImmediate);assert.equal(saved.length,1);assert.equal(saved[0][1],'correct secret');
});
test('disconnect fences slow password retrieval; moving other members never uses remembered credentials',async()=>{
  let resolve;const sent=[],asked=[];let loads=0;const moves=new ChannelMoves(v=>sent.push(v),v=>asked.push(v),()=>{},()=>{},{load:()=>{loads++;return new Promise(r=>resolve=r);},save:async()=>{},forget:async()=>{}});
  moves.start({channel:2,name:'Room',context:'scope'});moves.result({id:sent[0].id,ok:false,code:781});moves.reset();resolve('stale password');await new Promise(setImmediate);assert.equal(sent.length,1);assert.equal(asked.length,0);
  moves.start({channel:2,name:'Room',context:'scope',client:99});moves.result({id:sent[1].id,ok:false,code:781});assert.equal(loads,1);assert.equal(asked.length,1);
});
