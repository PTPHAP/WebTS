import test from 'node:test';
import assert from 'node:assert/strict';
import {worklet} from './audio-worklet.mjs';
const energy=a=>a.reduce((sum,v)=>sum+v*v,0);
async function denoise(input,keyboard){const rn=await worklet('rnnoise',{preserveInput:keyboard}),gt=keyboard?await worklet('gtcrn'):undefined;try{return rn.process(gt?gt.process(input):input);}finally{assert.equal(rn.destroy(),false);if(gt)assert.equal(gt.destroy(),false);}}
test('real local keyboard chain suppresses deterministic broadband clicks more than RNNoise alone',async()=>{
  let seed=17;const input=Float32Array.from({length:48000*3},(_,i)=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;const phase=i%4800;return phase<900?((seed/4294967296)*2-1)*0.12*Math.exp(-phase/180):0;});
  const plain=await denoise(input,false),enhanced=await denoise(input,true);
  assert.ok(enhanced.every(Number.isFinite));assert.ok(10*Math.log10(energy(plain)/energy(enhanced))>6,'keyboard model must add at least 6 dB click suppression to this fixture');
});
test('quiet synthetic voiced sound remains finite and audible; no RMS gate is applied',async()=>{
  let phase=0;const input=Float32Array.from({length:48000*3},(_,i)=>{const time=i/48000;phase+=2*Math.PI*(150+30*Math.sin(time*12))/48000;let value=0;for(let harmonic=1;harmonic<16;harmonic++)value+=Math.sin(phase*harmonic)/harmonic;return value*0.015*(0.3+0.7*Math.sin(time*9)**2);});
  const output=await denoise(input,true);assert.ok(output.every(Number.isFinite));assert.ok(energy(output)>energy(input)*0.2,'quiet voiced fixture must not be gated to silence');
});
