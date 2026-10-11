import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/channel-folds.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const {toggleChannelFold}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
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
