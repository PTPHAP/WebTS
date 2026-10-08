import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
export async function worklet(model) {
  const source=await readFile(new URL(`../public/audio/${model}-0.4.1-ready1.js`,import.meta.url),'utf8');
  const wasm=await readFile(new URL(`../public/audio/0.4.1-${model}.wasm`,import.meta.url));
  let Processor,receive,resolve,reject;
  const ready=new Promise((r,j)=>{resolve=r;reject=j;});
  vm.runInNewContext(source.replaceAll('import.meta.url',"'https://fixture.example/audio/test.js'"),{WebAssembly,TextEncoder,TextDecoder,console,sampleRate:48000,
    AudioWorkletProcessor:class{port={addEventListener:(_,f)=>receive=f,postMessage:e=>e.type==='ready'?resolve():reject(Error('initialization failed'))};},registerProcessor:(_,p)=>Processor=p});
  const processor=new Processor({processorOptions:{maxChannels:1,wasmBinary:wasm.buffer.slice(wasm.byteOffset,wasm.byteOffset+wasm.byteLength)}});
  const timer=setTimeout(()=>reject(Error('ready timeout')),3000);
  try{await ready;}finally{clearTimeout(timer);}
  return {process(input){const out=new Float32Array(input.length);for(let p=0;p<input.length;p+=128){processor.process([[input.subarray(p,p+128)]],[[out.subarray(p,p+128)]],{});}return out;},destroy(){receive({data:'destroy'});return processor.process([],[],{});}};
}
