import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

test('real RNNoise WASM acknowledges ready, processes finite samples and destroys', async()=>{
  const source=await readFile(new URL('../public/audio/rnnoise-0.4.1-voice4.js',import.meta.url),'utf8');
  const wasm=await readFile(new URL('../public/audio/0.4.1-rnnoise.wasm',import.meta.url));
  let Processor,receive;
  let resolveReady,rejectReady;
  const ready=new Promise((resolve,reject)=>{resolveReady=resolve;rejectReady=reject;});
  const context={WebAssembly,TextEncoder,TextDecoder,Uint8Array,Float32Array,ArrayBuffer,console,
    AudioWorkletProcessor:class {port={addEventListener:(_,callback)=>{receive=callback;},postMessage:event=>event.type==='ready'?resolveReady():rejectReady(new Error('initialization failed'))};},
    registerProcessor:(_,type)=>{Processor=type;}
  };
  vm.runInNewContext(source.replaceAll('import.meta.url',"'https://fixture.example/audio/rnnoise.js'"),context);
  const processor=new Processor({processorOptions:{maxChannels:1,wasmBinary:wasm.buffer.slice(wasm.byteOffset,wasm.byteOffset+wasm.byteLength)}});
  await Promise.race([ready,new Promise((_,reject)=>{const timer=setTimeout(()=>reject(new Error('RNNoise ready timeout')),3000);timer.unref();})]);
  let peak=0;
  for(let block=0;block<160;block++) {
    const input=Float32Array.from({length:128},(_,i)=>Math.sin((block*128+i)*2*Math.PI*440/48000)*0.2),output=new Float32Array(128);
    assert.equal(processor.process([[input]],[[output]],{}),true);
    for(const value of output){assert.ok(Number.isFinite(value));peak=Math.max(peak,Math.abs(value));}
  }
  assert.ok(peak>0.001,'initialized processor must not remain silently zero');
  receive({data:'destroy'});assert.equal(processor.process([],[],{}),false);
});

test('invalid RNNoise WASM acknowledges failure instead of silently staying active',async()=>{
  const source=await readFile(new URL('../public/audio/rnnoise-0.4.1-voice4.js',import.meta.url),'utf8');
  let Processor,signal;
  const failed=new Promise(resolve=>{signal=resolve;});
  vm.runInNewContext(source.replaceAll('import.meta.url',"'https://fixture.example/audio/rnnoise.js'"),{WebAssembly,TextEncoder,TextDecoder,console:{warn(){},error(){}},
    AudioWorkletProcessor:class {port={addEventListener(){},postMessage:signal};},registerProcessor:(_,type)=>{Processor=type;}});
  new Processor({processorOptions:{maxChannels:1,wasmBinary:new ArrayBuffer(0)}});
  const result=await Promise.race([failed,new Promise((_,reject)=>{const timer=setTimeout(()=>reject(new Error('failure acknowledgement timeout')),3000);timer.unref();})]);
  assert.equal(result.type,'error');
});
