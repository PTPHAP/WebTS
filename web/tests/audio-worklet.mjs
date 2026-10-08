import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
export async function worklet(model,options={},upstream=false) {
  let source=await readFile(new URL(upstream?`../node_modules/@sapphi-red/web-noise-suppressor/dist/${model}/workletProcessor.js`:`../public/audio/${model}-0.4.1-${model==='rnnoise'?'voice3':'strength1'}.js`,import.meta.url),'utf8');
  if(upstream)source=source.replace('this.destroyed&&this.destroy()',"this.destroyed&&this.destroy(),this.port.postMessage({type:'ready'})").replace('})()}process',"})().catch(()=>this.port.postMessage({type:'error'}))}process");
  const wasm=await readFile(new URL(`../public/audio/0.4.1-${model}.wasm`,import.meta.url));
  const events=[];let Processor,receive,resolve,reject;
  const ready=new Promise((r,j)=>{resolve=r;reject=j;});
  vm.runInNewContext(source.replaceAll('import.meta.url',"'https://fixture.example/audio/test.js'"),{WebAssembly,TextEncoder,TextDecoder,console,sampleRate:48000,
    AudioWorkletProcessor:class{port={addEventListener:(_,f)=>receive=f,postMessage:e=>e.type==='ready'?resolve():e.type==='speech'?events.push(e):reject(Error('initialization failed'))};},registerProcessor:(_,p)=>Processor=p});
  const processor=new Processor({processorOptions:{maxChannels:1,wasmBinary:wasm.buffer.slice(wasm.byteOffset,wasm.byteOffset+wasm.byteLength),...options}});
  const timer=setTimeout(()=>reject(Error('ready timeout')),3000);
  try{await ready;}finally{clearTimeout(timer);}
  return {events,message(data){receive({data});},process(input){const out=new Float32Array(input.length);for(let p=0;p<input.length;p+=128){processor.process([[input.subarray(p,p+128)]],[[out.subarray(p,p+128)]],{});}return out;},destroy(){receive({data:'destroy'});return processor.process([],[],{});}};
}
