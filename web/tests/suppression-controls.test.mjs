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
 try{const warm=noise(48000*2);baseline.process(warm);typing.process(warm);typing.message({type:'typing'});const input=noise(9600),plain=baseline.process(input),quiet=typing.process(input);assert.ok(energy(quiet)<energy(plain)*.3);}
 finally{baseline.destroy();typing.destroy();}
});

test('typing/click suppression leaves an active quiet voice stream intact',async()=>{
 let phase=0;const voice=Float32Array.from({length:48000*2},(_,i)=>{const t=i/48000;phase+=2*Math.PI*(150+30*Math.sin(t*12))/48000;let value=0;for(let h=1;h<16;h++)value+=Math.sin(phase*h)/h;return value*.015*(.3+.7*Math.sin(t*9)**2);});
 const reference=await worklet('rnnoise',{preserveInput:true}),keyed=await worklet('rnnoise',{preserveInput:true});
 try{reference.process(voice);keyed.process(voice);assert.equal(keyed.events.at(-1).enabled,true);keyed.message({type:'typing'});const tail=voice.slice(0,9600);assert.deepEqual(keyed.process(tail),reference.process(tail),'a keyboard/mouse timing hint must not attenuate the ongoing voice');}
 finally{reference.destroy();keyed.destroy();}
});
