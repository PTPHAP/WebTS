import test from 'node:test';
import assert from 'node:assert/strict';
import {worklet} from './audio-worklet.mjs';
import {createVoiceGate} from '../scripts/voice-gate.mjs';

test('model probability, lookback, hysteresis and smooth tail preserve quiet speech without a level gate',()=>{
  const gate=createVoiceGate(),frame=probability=>{const samples=new Float32Array(480).fill(.00001);gate(samples,probability);return samples;};
  assert.ok(frame(.1).every(v=>v===0));const attack=frame(.9);assert.ok(attack[0]>0&&attack[479]===Math.fround(.00001));assert.ok(frame(.4).every(v=>v>0),'soft syllables sustain an open voice stream');
  for(let i=0;i<19;i++)assert.ok(frame(.1).some(v=>v>0),'short tail protects speech ends');
  const tail=frame(.1);assert.ok(tail[0]>tail[479],'fade avoids an abrupt cut');assert.ok(frame(.1).every(v=>v===0));assert.throws(()=>frame(NaN),/probability/);
});

test('disabled voice-only mode preserves the original continuous denoiser output',async()=>{
  const input=Float32Array.from({length:48000},(_,i)=>Math.sin(i*2*Math.PI*150/48000)*.03),legacy=await worklet('rnnoise',{},true),disabled=await worklet('rnnoise',{voiceOnly:false});
  try{assert.deepEqual(disabled.process(input),legacy.process(input));}finally{legacy.destroy();disabled.destroy();}
});

test('recognition-only mode preserves input samples; fallback restores actual RNNoise filtering',async()=>{
  let seed=61;const input=Float32Array.from({length:48000*2},()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return (seed/4294967296-.5)*.02;});
  const analysis=await worklet('rnnoise',{preserveInput:true}),filter=await worklet('rnnoise');
  try{const passthrough=analysis.process(input);let matched=false;for(let delay=0;delay<1600;delay+=16){if(passthrough.slice(3200,6400).every((v,i)=>v===input[3200+i-delay])){matched=true;break;}}assert.ok(matched,'classification must not filter GTCRN audio twice');analysis.message({type:'preserve',enabled:false});const clean=analysis.process(input),reference=filter.process(input);const energy=a=>a.reduce((s,v)=>s+v*v,0);assert.ok(energy(clean.slice(48000))<energy(passthrough.slice(48000))*.85);assert.ok(energy(reference)>0);}
  finally{analysis.destroy();filter.destroy();}
});

test('voice-only mode rejects settled fan hiss and hum without an amplitude threshold',async()=>{
  let seed=29;
  const input=Float32Array.from({length:48000*4},(_,i)=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return (seed/4294967296*2-1)*.004+Math.sin(i*2*Math.PI*60/48000)*.006;});
  const processor=await worklet('rnnoise',{voiceOnly:true});
  try{const output=processor.process(input);assert.ok(output.every(Number.isFinite));assert.ok(output.slice(48000*2).every(v=>v===0),'no speech: residual noise must become silence after settling');}
  finally{assert.equal(processor.destroy(),false);}
});

test('real keyboard plus voice-only chain preserves quiet voiced input then settles to silence',async()=>{
  let seed=31,phase=0;
  const input=Float32Array.from({length:48000*5},(_,i)=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;const time=i/48000;let value=(seed/4294967296*2-1)*.002;if(time>=1&&time<3){phase+=2*Math.PI*(150+30*Math.sin(time*12))/48000;for(let harmonic=1;harmonic<16;harmonic++)value+=Math.sin(phase*harmonic)/harmonic*.015*(.3+.7*Math.sin(time*9)**2);}return value;});
  const gt=await worklet('gtcrn'),plain=await worklet('rnnoise',{preserveInput:true}),voice=await worklet('rnnoise',{voiceOnly:true,preserveInput:true});
  try{const enhanced=gt.process(input),reference=plain.process(enhanced),output=voice.process(enhanced);const energy=a=>a.reduce((sum,v)=>sum+v*v,0);assert.ok(energy(output.slice(48000,48000*3.3))>energy(reference.slice(48000,48000*3.3))*.8,'quiet voiced energy must remain audible');assert.deepEqual(voice.events.map(e=>e.enabled),[true,false],'real model posts only speech boundaries, never per-frame telemetry');assert.ok(output.slice(48000*4).every(v=>v===0),'pause settles to digital silence');}
  finally{gt.destroy();plain.destroy();voice.destroy();}
});
