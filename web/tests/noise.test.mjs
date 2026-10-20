import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

test('real RNNoise WASM acknowledges ready, processes finite samples and destroys', async()=>{
  const source=await readFile(new URL('../public/audio/rnnoise-0.4.1-voice6.js',import.meta.url),'utf8');
  const wasm=await readFile(new URL('../public/audio/0.4.1-rnnoise.wasm',import.meta.url));
  let Processor,receive,started=false;const acknowledgements=[];
  let resolveReady,rejectReady;
  const ready=new Promise((resolve,reject)=>{resolveReady=resolve;rejectReady=reject;});
  const context={WebAssembly,TextEncoder,TextDecoder,Uint8Array,Float32Array,ArrayBuffer,console,
    AudioWorkletProcessor:class {port={addEventListener:(_,callback)=>{receive=callback;},start(){started=true;},postMessage:event=>{if(event.type==='ready')resolveReady();else if(event.type==='preserve')acknowledgements.push(event.enabled);else if(event.type==='error')rejectReady(new Error('initialization failed'));}};},
    registerProcessor:(_,type)=>{Processor=type;}
  };
  vm.runInNewContext(source.replaceAll('import.meta.url',"'https://fixture.example/audio/rnnoise.js'"),context);
  const processor=new Processor({processorOptions:{maxChannels:1,wasmBinary:wasm.buffer.slice(wasm.byteOffset,wasm.byteOffset+wasm.byteLength)}});
  await Promise.race([ready,new Promise((_,reject)=>{const timer=setTimeout(()=>reject(new Error('RNNoise ready timeout')),3000);timer.unref();})]);
  assert.equal(started,true,'processor MessagePort must dispatch real browser controls');
  let peak=0;
  for(let block=0;block<160;block++) {
    const input=Float32Array.from({length:128},(_,i)=>Math.sin((block*128+i)*2*Math.PI*440/48000)*0.2),output=new Float32Array(128);
    assert.equal(processor.process([[input]],[[output]],{}),true);
    for(const value of output){assert.ok(Number.isFinite(value));peak=Math.max(peak,Math.abs(value));}
  }
  assert.ok(peak>0.001,'initialized processor must not remain silently zero');
  receive({data:{type:'preserve',enabled:true}});
  for(let i=0;i<20;i++)processor.process([[new Float32Array(128).fill(.314159)]],[[new Float32Array(128)]],{});
  receive({data:{type:'preserve',enabled:false}});assert.deepEqual(acknowledgements,[true,false]);
  let rawHistory=0;
  for(let i=0;i<12;i++){const fallback=new Float32Array(128);processor.process([[new Float32Array(128)]],[[fallback]],{});rawHistory+=fallback.filter(value=>Math.abs(value-.314159)<1e-7).length;}
  assert.equal(rawHistory,0,'acknowledged denoising cannot replay the raw recognition-only ring buffer');
  receive({data:'destroy'});assert.equal(processor.process([],[],{}),false);
});

test('invalid RNNoise WASM acknowledges failure instead of silently staying active',async()=>{
  const source=await readFile(new URL('../public/audio/rnnoise-0.4.1-voice6.js',import.meta.url),'utf8');
  let Processor,signal;
  const failed=new Promise(resolve=>{signal=resolve;});
  vm.runInNewContext(source.replaceAll('import.meta.url',"'https://fixture.example/audio/rnnoise.js'"),{WebAssembly,TextEncoder,TextDecoder,console:{warn(){},error(){}},
    AudioWorkletProcessor:class {port={addEventListener(){},start(){},postMessage:signal};},registerProcessor:(_,type)=>{Processor=type;}});
  new Processor({processorOptions:{maxChannels:1,wasmBinary:new ArrayBuffer(0)}});
  const result=await Promise.race([failed,new Promise((_,reject)=>{const timer=setTimeout(()=>reject(new Error('failure acknowledgement timeout')),3000);timer.unref();})]);
  assert.equal(result.type,'error');
});
