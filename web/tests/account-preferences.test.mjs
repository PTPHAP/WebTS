import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
async function fixture(file,api=async()=>{}){
  const timers=new Map(),events=new Map(),cleanups=[],states=[];let counter=0;
  const eventTarget={addEventListener:(name,fn)=>{events.set(name,fn);},removeEventListener:(name,fn)=>{if(events.get(name)===fn)events.delete(name);}};
  const document={...eventTarget,visibilityState:'visible',hidden:false,hasFocus:()=>true};
  const context=vm.createContext({exports:{},document,window:eventTarget,setTimeout:(fn,ms)=>{timers.set(++counter,{fn,ms});return counter;},clearTimeout:id=>timers.delete(id),setInterval:()=>++counter,clearInterval(){},require:name=>name==='react'?{useRef:value=>({current:value}),useState:initial=>{const state={value:typeof initial==='function'?initial():initial};states.push(state);return[state.value,value=>{state.value=value;}];},useEffect:fn=>cleanups.push(fn())}:{api}});
  const source=await readFile(new URL(`../src/${file}`,import.meta.url),'utf8');vm.runInContext(ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,context);
  const drain=async()=>{for(let i=0;i<15;i++)await Promise.resolve();};
  return {module:context.exports,timers,events,states,document,cleanups,drain,async tick(){const [id,{fn}]=[...timers][0];timers.delete(id);fn();await drain();}};
}
test('remote preferences update all mounted views and dirty fields merge without clobbering unrelated settings',async()=>{
  let saved={burn:true,burnSeconds:600,theme:'dark'},calls=[];const f=await fixture('account-preferences.ts',async(path,body)=>{calls.push(body);if(body)saved={...saved,...body.patch};return {account:1,initialized:true,preferences:saved};});
  const [,burn]=f.module.useAccountPreference('burn',false);f.module.useAccountPreference('burn',false);f.module.useAccountPreference('theme','light');const stop=f.module.startPreferences(1,assert.fail);await f.drain();assert.equal(f.states[0].value,true);assert.equal(f.states[1].value,true);assert.equal(f.states[2].value,'dark');burn(false);await f.tick();assert.equal(saved.burn,false);assert.equal(saved.burnSeconds,600);assert.deepEqual(JSON.parse(JSON.stringify(calls.at(-1))),{account:1,patch:{burn:false}});stop();
});
test('stale previous-account responses cannot populate a new login or write under its cookie',async()=>{
  let complete;const calls=[];const f=await fixture('account-preferences.ts',(path,body)=>{calls.push(body);if(calls.length===1)return new Promise(done=>complete=done);return Promise.resolve({account:2,initialized:true,preferences:{burn:false}});});const [,burn]=f.module.useAccountPreference('burn',false);const old=f.module.startPreferences(1,assert.fail);old();const stop=f.module.startPreferences(2,assert.fail);await f.drain();complete({account:1,initialized:true,preferences:{burn:true}});await f.drain();assert.equal(f.states[0].value,false);burn(true);await f.tick();assert.equal(calls.at(-1).account,2);stop();
});
test('failed saves retain the newest field value and retry with bounded delay',async()=>{
  let attempts=0,last,errors=0;const f=await fixture('account-preferences.ts',async(path,body)=>{if(body){last=body;if(++attempts===1)throw Error('offline');}return {account:1,initialized:true,preferences:body?.patch??{burn:false}};});const [,burn]=f.module.useAccountPreference('burn',false);const stop=f.module.startPreferences(1,()=>errors++);await f.drain();burn(true);await f.tick();assert.equal(errors,1);assert.equal([...f.timers.values()][0].ms,5000);burn(false);await f.tick();assert.equal(last.patch.burn,false);assert.equal(f.states[0].value,false);stop();
});
test('private-screen mask follows background and focus changes and unregisters handlers',async()=>{
  const f=await fixture('private-screen.ts');assert.equal(f.module.usePrivateScreen(),false);f.document.hasFocus=()=>false;f.events.get('blur')();assert.equal(f.states[0].value,true);f.document.hasFocus=()=>true;f.events.get('focus')();assert.equal(f.states[0].value,false);f.document.hidden=true;f.events.get('visibilitychange')();assert.equal(f.states[0].value,true);f.cleanups.forEach(fn=>fn?.());assert.equal(f.events.size,0);
});
