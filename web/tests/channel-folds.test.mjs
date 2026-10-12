import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/channel-folds.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const {toggleChannelFold,foldedChannelMembers}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('folded avatar summaries include only supplied members in this visible channel subtree',()=>{
 const channels=[{id:1,parent:0},{id:2,parent:1},{id:3,parent:2},{id:4,parent:0}];
 const members=[{id:10,channel:1},{id:11,channel:3},{id:12,channel:4},{id:13,channel:999}];
 assert.deepEqual(foldedChannelMembers(channels,members,1),members.slice(0,2));
 assert.deepEqual(foldedChannelMembers(channels,members,4),[members[2]]);
 assert.deepEqual(foldedChannelMembers(channels,members,999),[]);
 assert.deepEqual(foldedChannelMembers([{id:1,parent:2},{id:2,parent:1}],members,1),[members[0]]);
 assert.deepEqual(channels,[{id:1,parent:0},{id:2,parent:1},{id:3,parent:2},{id:4,parent:0}]);
});
test('channel folds toggle independently per server without mutating earlier preferences',()=>{
  const old={a:[1,2],b:[1]};const next=toggleChannelFold(old,'a',1);
  assert.deepEqual(old,{a:[1,2],b:[1]});assert.deepEqual(next,{b:[1],a:[2]});
  assert.deepEqual(toggleChannelFold(next,'a',2),{b:[1]});
});
test('fold history is bounded even after many servers and channels',()=>{
  let value={};for(let s=0;s<20;s++)for(let c=1;c<=300;c++)value=toggleChannelFold(value,`s${s}`,c);
  assert.equal(Object.keys(value).length,4);
  for(const ids of Object.values(value)){assert.equal(ids.length,128);assert.equal(new Set(ids).size,128);assert.equal(ids.at(-1),300);}
});

test('server names resembling object properties cannot inherit arrays or alter object prototypes',()=>{
  for(const server of ['constructor','toString','__proto__']){
    const next=toggleChannelFold({},server,1);
    assert.equal(Object.hasOwn(next,server),true);assert.deepEqual(next[server],[1]);
    assert.deepEqual(toggleChannelFold(next,server,1),{});
    assert.equal(Object.getPrototypeOf(next),Object.prototype);
  }
});
