import test from 'node:test';
import assert from 'node:assert/strict';
import {worklet} from './audio-worklet.mjs';
const energy=a=>a.reduce((s,v)=>s+v*v,0);
function noise(length){let seed=83;return Float32Array.from({length},()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return (seed/4294967296-.5)*.008;});}
test('zero suppression strength preserves aligned dry samples for both fixed models',async()=>{
 const input=noise(48000);
 for(const [model,delay] of [['gtcrn',1530],['rnnoise',992]]){
  const p=await worklet(model,{strength:0});try{const output=p.process(input);assert.deepEqual(output.slice(3200),input.slice(3200-delay,input.length-delay));}finally{p.destroy();}
 }
});
test('local key event suppresses nonvoice noise even when continuous denoising is selected',async()=>{
 const baseline=await worklet('rnnoise',{preserveInput:true}),typing=await worklet('rnnoise',{preserveInput:true});
 try{const warm=noise(48000);baseline.process(warm);typing.process(warm);typing.message({type:'typing'});const input=noise(9600),plain=baseline.process(input),quiet=typing.process(input);assert.ok(energy(quiet)<energy(plain)*.3);}
 finally{baseline.destroy();typing.destroy();}
});
