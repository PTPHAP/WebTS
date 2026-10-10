import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
const source=await readFile(new URL('../src/Friends.tsx',import.meta.url),'utf8');
const callback=source.match(/setInterval\(\(\)=>\{setRecords\((.+)\);\},1000\)/)[1];
const expire=vm.runInNewContext(`(${callback})`,{Date:{now:()=>100000}});

test('expiry clock preserves unchanged message state and removes only expired records',()=>{
  const live={expires:110},burning={expires:120,burnAt:100001},expired={expires:100},burnt={expires:110,burnAt:100000};
  const unchanged=[live,burning];
  assert.equal(expire(unchanged),unchanged);
  const mixed=[live,burning,expired,burnt];
  assert.deepEqual(Array.from(expire(mixed)),unchanged);
  assert.equal(mixed.length,4);
});
