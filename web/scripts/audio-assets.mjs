import {readFile,writeFile,copyFile,mkdir,unlink} from 'node:fs/promises';
import {createVoiceGate,createVoiceActivity} from './voice-gate.mjs';
const packageRoot=new URL('../node_modules/@sapphi-red/web-noise-suppressor/dist/',import.meta.url);
const target=new URL('../public/audio/',import.meta.url);
await mkdir(target,{recursive:true});
// Cached pages still request the exact voice6 processor after a clean deployment.
await copyFile(new URL('../compat/audio/rnnoise-0.4.1-voice6.js',import.meta.url),new URL('rnnoise-0.4.1-voice6.js',target));
for(const model of ['rnnoise','gtcrn']) {
  let source=await readFile(new URL(`${model}/workletProcessor.js`,packageRoot),'utf8');
  // Pin every patch seam; do not silently patch a different upstream release.
  // Typing ducking is capped at 6 dB: detector lookback must not erase a quiet
  // first phoneme. GTCRN/RNNoise remain responsible for actual noise removal.
  if(model==='rnnoise'){
    const edits=[
      ['const u=e=>',`const webtsVoiceGate=${createVoiceGate.toString()};const webtsVoiceActivity=${createVoiceActivity.toString()};const u=e=>`],
      ['f=e=>{let t=e.createDenoiseState(),n=e=>{u(e),t.processFrame(e),d(e)}','f=(e,voiceOnly,onSpeech,preserveInput,strength,control)=>{let t=e.createDenoiseState(),original=preserveInput?new Float32Array(480):null,raw=new Float32Array(480),previousRaw=new Float32Array(480),typingGain=1,speaking=false,gate=voiceOnly?webtsVoiceGate():null,activity=webtsVoiceActivity(onSpeech,control.onPrepare),n=e=>{raw.set(e);original?.set(e);u(e);const probability=t.processFrame(e);speaking=activity(probability);d(e);if(original)e.set(original);else if(strength<1)for(let i=0;i<480;i++)e[i]=e[i]*strength+previousRaw[i]*(1-strength);previousRaw.set(raw);const typing=control.typing>0&&!speaking;control.typing=Math.max(0,control.typing-1);if(!original||voiceOnly)for(let i=0;i<480;i++){typingGain=typing?Math.max(.5,typingGain-1/240):Math.min(1,typingGain+1/480);e[i]*=typingGain;}gate?.(e,probability)}'],
      ['p=(e,{bufferSize:t,maxChannels:n})=>','p=(e,{bufferSize:t,maxChannels:n,voiceOnly,onSpeech,preserveInput,strength,control})=>'],
      ['()=>f(e)','()=>f(e,voiceOnly,onSpeech,preserveInput,strength,control)'],
      // With GTCRN, RNNoise only classifies speech: do not buffer its unchanged PCM again.
      ['let s=(a+1280)%r;t.set(i.subarray(s,s+128))','if(original&&!voiceOnly){t.set(e);const typing=control.typing>0&&!speaking;for(let j=0;j<t.length;j++){typingGain=typing?Math.max(.5,typingGain-1/240):Math.min(1,typingGain+1/480);t[j]*=typingGain;}}else{let s=(a+1280)%r;t.set(i.subarray(s,s+128))}'],
      ['destroy:()=>{t.destroy()}}},p=', 'setPreserve:value=>{original=value?new Float32Array(480):null;i.fill(0);previousRaw.fill(0)},destroy:()=>{t.destroy()}}},p='],
      ['destroy:()=>{for(let e of r)e.destroy()}}};','setPreserve:value=>{for(let e of r)e.setPreserve(value)},destroy:()=>{for(let e of r)e.destroy()}}};'],
      ['this.destroyed=!1,this.port.addEventListener','this.destroyed=!1,this.control={typing:0,onPrepare:enabled=>this.port.postMessage({type:"prepare",enabled})},this.port.addEventListener'],
      ['e.data===`destroy`&&this.destroy()', 'e.data===`destroy`&&this.destroy();if(e.data?.type===`preserve`&&this.processor){this.processor.setPreserve(e.data.enabled===true);this.port.postMessage({type:"preserve",enabled:e.data.enabled===true});}if(e.data?.type===`typing`)this.control.typing=20'],
      ['maxChannels:e.processorOptions.maxChannels}),this.destroyed','maxChannels:e.processorOptions.maxChannels,voiceOnly:e.processorOptions.voiceOnly===true,preserveInput:e.processorOptions.preserveInput===true,strength:e.processorOptions.strength??1,control:this.control,onSpeech:enabled=>this.port.postMessage({type:"speech",enabled})}),this.destroyed'],
    ];
    for(const [before,after] of edits){if(source.split(before).length!==2)throw new Error('RNNoise speech gate seam changed');source=source.replace(before,after);}
  } else {
    // This pinned 48 kHz wrapper has 1530 samples of signal delay, including resampling.
    // Align dry audio before mixing strength, avoiding the comb filtering of unaligned blending.
    const edits=[
      ['this.destroyed=!1,this.port.addEventListener','this.destroyed=!1,this.strength=e.processorOptions.strength??1,this.dry=new Float32Array(1530),this.position=0,this.port.addEventListener'],
      ['this.processor.process(e[0],t[0])','(this.processor.process(e[0],t[0]),this.mix(e[0][0],t[0][0]))'],
      ['destroy(){this.destroyed=!0','mix(input,output){if(this.strength>=1)return;for(let i=0;i<output.length;i++){const dry=this.dry[this.position];this.dry[this.position]=input[i];this.position=(this.position+1)%this.dry.length;output[i]=output[i]*this.strength+dry*(1-this.strength);}}destroy(){this.destroyed=!0'],
    ];
    for(const [before,after] of edits){if(source.split(before).length!==2)throw new Error('GTCRN strength seam changed');source=source.replace(before,after);}
  }
  // addEventListener does not enable a MessagePort queue. Preserve/typing/destroy
  // controls must actually reach the processor in a browser, not only in VM tests.
  const portStart='}),(async()=>{let t=await';
  if(source.split(portStart).length!==2)throw new Error(`${model} message-port seam changed`);
  source=source.replace(portStart,'}),this.port.start(),(async()=>{let t=await');
  const marker='this.destroyed&&this.destroy()';
  if(source.split(marker).length!==2)throw new Error(`${model} source changed; review ready acknowledgement patch`);
  source=source.replace(marker,`${marker},this.port.postMessage({type:'ready'})`);
  const end='})()}process';
  if(source.split(end).length!==2)throw new Error(`${model} initialization source changed`);
  source=source.replace(end,"})().catch(()=>this.port.postMessage({type:'error'}))}process");
  source=source.replace(/\/\/# sourceMappingURL=.*$/m,'');
  if(model==='rnnoise'){
    // Keep the currently deployed component available for pages opened before this upgrade.
    await writeFile(new URL('rnnoise-0.4.1-voice7.js',target),source);
    const copyEdits=[
      ['raw=new Float32Array(480),previousRaw=new Float32Array(480)','raw=strength<1?new Float32Array(480):null,previousRaw=strength<1?new Float32Array(480):null'],
      ['raw.set(e);original?.set(e)','raw?.set(e);original?.set(e)'],
      ['previousRaw.set(raw);','if(raw){const previous=previousRaw;previousRaw=raw;raw=previous;}'],
      ['previousRaw.fill(0)','previousRaw?.fill(0)'],
    ];
    for(const [before,after] of copyEdits){if(source.split(before).length!==2)throw new Error('RNNoise copy optimization seam changed');source=source.replace(before,after);}
  }
  await writeFile(new URL(`${model}-0.4.1-${model==='rnnoise'?'voice8':'strength2'}.js`,target),source);
}
for(const obsolete of ['rnnoise-0.4.1-ready1.js','rnnoise-0.4.1-voice1.js','rnnoise-0.4.1-voice2.js','rnnoise-0.4.1-voice3.js','rnnoise-0.4.1-voice4.js','rnnoise-0.4.1-voice5.js','gtcrn-0.4.1-ready1.js','gtcrn-0.4.1-strength1.js'])await unlink(new URL(obsolete,target)).catch(error=>{if(error.code!=='ENOENT')throw error;});
for(const file of ['rnnoise.wasm','rnnoise_simd.wasm','gtcrn.wasm'])await copyFile(new URL(file,packageRoot),new URL(`0.4.1-${file}`,target));
await copyFile(new URL('../node_modules/@shiguredo/rnnoise-wasm/dist/rnnoise.js',import.meta.url),new URL('rnnoise-2025.1.5.js',target));
await copyFile(new URL('../workers/realtime-noise.js',import.meta.url),new URL('realtime-noise-v1.js',target));
await writeFile(new URL('voice-gate-v1.js',target),`export const createVoiceGate=${createVoiceGate.toString()};\n`);
