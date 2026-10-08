import {readFile,writeFile,copyFile,mkdir,unlink} from 'node:fs/promises';
import {createVoiceGate} from './voice-gate.mjs';
const packageRoot=new URL('../node_modules/@sapphi-red/web-noise-suppressor/dist/',import.meta.url);
const target=new URL('../public/audio/',import.meta.url);
await mkdir(target,{recursive:true});
for(const model of ['rnnoise','gtcrn']) {
  let source=await readFile(new URL(`${model}/workletProcessor.js`,packageRoot),'utf8');
  // Pin every patch seam; do not silently patch a different upstream release.
  if(model==='rnnoise'){
    const edits=[
      ['const u=e=>',`const webtsVoiceGate=${createVoiceGate.toString()};const u=e=>`],
      ['f=e=>{let t=e.createDenoiseState(),n=e=>{u(e),t.processFrame(e),d(e)}','f=(e,voiceOnly)=>{let t=e.createDenoiseState(),gate=voiceOnly?webtsVoiceGate():null,n=e=>{u(e);const probability=t.processFrame(e);d(e);gate?.(e,probability)}'],
      ['p=(e,{bufferSize:t,maxChannels:n})=>','p=(e,{bufferSize:t,maxChannels:n,voiceOnly})=>'],
      ['()=>f(e)','()=>f(e,voiceOnly)'],
      ['maxChannels:e.processorOptions.maxChannels}),this.destroyed','maxChannels:e.processorOptions.maxChannels,voiceOnly:e.processorOptions.voiceOnly===true}),this.destroyed'],
    ];
    for(const [before,after] of edits){if(source.split(before).length!==2)throw new Error('RNNoise speech gate seam changed');source=source.replace(before,after);}
  }
  const marker='this.destroyed&&this.destroy()';
  if(source.split(marker).length!==2)throw new Error(`${model} source changed; review ready acknowledgement patch`);
  source=source.replace(marker,`${marker},this.port.postMessage({type:'ready'})`);
  const end='})()}process';
  if(source.split(end).length!==2)throw new Error(`${model} initialization source changed`);
  source=source.replace(end,"})().catch(()=>this.port.postMessage({type:'error'}))}process");
  source=source.replace(/\/\/# sourceMappingURL=.*$/m,'');
  await writeFile(new URL(`${model}-0.4.1-${model==='rnnoise'?'voice1':'ready1'}.js`,target),source);
}
await unlink(new URL('rnnoise-0.4.1-ready1.js',target)).catch(error=>{if(error.code!=='ENOENT')throw error;});
for(const file of ['rnnoise.wasm','rnnoise_simd.wasm','gtcrn.wasm'])await copyFile(new URL(file,packageRoot),new URL(`0.4.1-${file}`,target));
