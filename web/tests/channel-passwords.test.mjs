import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
import {webcrypto} from 'node:crypto';
import vm from 'node:vm';
const source=await readFile(new URL('../src/channel-passwords.ts',import.meta.url),'utf8');
const code=ts.transpileModule(source.replace(/^import .*$/mg,'').replace('export const channelPasswords','const channelPasswords')+'\nglobalThis.passwords=channelPasswords;',{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
test('passwords are AES-256 ciphertext scoped to account, server and channel; tampering and expiry fail closed',async()=>{
  const records=new Map();const context={crypto:webcrypto,TextEncoder,TextDecoder,Date,Uint8Array,navigator:{locks:{request:async(_,f)=>f()}},loadLocal:async(_,key)=>records.get(key),saveLocal:async(_,key,value)=>{if(value===undefined)records.delete(key);else records.set(key,value);}};vm.createContext(context);vm.runInContext(code,context);const p=context.passwords,target={channel:3,name:'Room',context:'account-a:server-uid:ts.example.invalid:9987'};
  await p.save(target,'synthetic channel password');assert.equal(await p.load(target),'synthetic channel password');const key=records.get(`channel-key:${target.context}`);assert.equal(key.extractable,false);assert.equal(key.algorithm.length,256);const name=`channel-password:${target.context}:3`,stored=records.get(name);assert(!JSON.stringify(stored).includes('synthetic channel password'));
  assert.equal(await p.load({...target,context:'account-b:server-uid:ts.example.invalid:9987'}),undefined);
  const copied=`channel-password:${target.context}:4`;records.set(copied,stored);assert.equal(await p.load({...target,channel:4}),undefined);assert(!records.has(copied));
  stored.expires=Date.now()-1;assert.equal(await p.load(target),undefined);assert(!records.has(name));await p.save(target,'replacement');await p.forget(target);assert.equal(await p.load(target),undefined);
});
