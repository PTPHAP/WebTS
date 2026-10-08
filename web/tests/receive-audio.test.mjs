import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/receive-audio.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {receiveGain}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
test('remote automatic gain lifts quiet speakers, softens loud speakers and preserves manual disable',()=>{
 let quiet=1,loud=1;for(let n=0;n<150;n++){quiet=receiveGain(.03,quiet,true);loud=receiveGain(.3,loud,true);}
 assert.ok(Math.abs(quiet-4)<.01);assert.ok(Math.abs(loud-.5)<.01);assert.equal(receiveGain(.01,quiet,false),1);
});
test('remote automatic gain never raises background hiss or pauses and rejects nonfinite levels',()=>{
 for(const rms of [0,.001,NaN,Infinity])assert.equal(receiveGain(rms,1,true),1);
 let boosted=4;for(let i=0;i<50;i++)boosted=receiveGain(.001,boosted,true);assert.ok(Math.abs(boosted-1)<.002);
 for(const rms of [.009,.01,.1,1,100]){let gain=1;for(let i=0;i<100;i++)gain=receiveGain(rms,gain,true);assert.ok(gain>=.5&&gain<=4);}
});
