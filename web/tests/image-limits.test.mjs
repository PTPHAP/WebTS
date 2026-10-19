import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/image-limits.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {defaultImageLimits,readImageLimits,imageLimitFields}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));

test('large native avatar and crop policies survive browser validation without removing bounds',()=>{
  assert.equal(readImageLimits({avatar_download_kib:2048}).avatar_download_kib,2048);
  assert.equal(readImageLimits({avatar_download_kib:4096,avatar_source_mib:32}).avatar_source_mib,32);
  for(const {key,min,max} of imageLimitFields){
    assert.ok(defaultImageLimits[key]>=min&&defaultImageLimits[key]<=max);
    for(const invalid of [min-1,max+1,Infinity,NaN,1.5,'2048'])assert.equal(readImageLimits({[key]:invalid})[key],defaultImageLimits[key]);
  }
});

test('browser defaults match server defaults for upgraded and new sites',async()=>{
  const rust=await readFile(new URL('../../server/src/settings.rs',import.meta.url),'utf8');
  const body=rust.split('impl Default for ImageLimits {')[1].split('impl ImageLimits {')[0];
  const values=Object.fromEntries([...body.matchAll(/(\w+): (\d+),/g)].map(([,key,n])=>[key,Number(n)]));
  assert.deepEqual(defaultImageLimits,values);
});
