import {Rnnoise} from './rnnoise-2025.1.5.js';
import {createVoiceGate} from './voice-gate-v1.js';
let model,settings,gate,dry,dryAt=0,typing=0,typingGain=1,hold=0,lead=0,floor=1e-7,speaking=false,preparing=false,started=false;
const bounded=(x,fallback,min,max)=>Number.isFinite(x)?Math.max(min,Math.min(max,x)):fallback;
function render(input){
 const output=Float32Array.from(input,(x,i)=>(x+(i%2?1:-1)*1e-8)*32767),probability=model.processFrame(output);
 if(!Number.isFinite(probability)||probability<0||probability>1)throw Error('Invalid model probability');
 let power=0;for(const x of input)power+=x*x;power/=480;
 if(probability<.5&&!speaking)floor+=(power-floor)*.02;
 lead=probability>=.9||power>Math.max(1e-6,floor*4)?3:Math.max(0,lead-1);
 const ready=speaking||lead>0;if(ready!==preparing){preparing=ready;postMessage({type:'prepare',enabled:ready});}
 hold=probability>=.99?40:Math.max(0,hold-1);const active=hold>0;
 if(active!==speaking){speaking=active;postMessage({type:'speech',enabled:active});}
 const duck=typing>0&&!speaking;typing=Math.max(0,typing-1);
 for(let i=0;i<480;i++){
  const delayed=dry[dryAt];dry[dryAt]=input[i];dryAt=(dryAt+1)%960;
  output[i]=output[i]/32767*settings.strength+delayed*(1-settings.strength);
  typingGain=duck?Math.max(.5,typingGain-1/240):Math.min(1,typingGain+1/480);output[i]*=typingGain;
 }
 gate?.(output,probability);for(let i=0;i<480;i++)output[i]*=settings.gain;
 return output;
}
onmessage=async({data})=>{
 try{
  if(data.type==='init'){
   if(model||started)throw Error('Duplicate initialization');
   const module=await Rnnoise.load();if(module.frameSize!==480)throw Error('Unsupported model frame');model=module.createDenoiseState();
   settings={strength:bounded(data.settings.strength,1,0,1),gain:bounded(data.settings.gain,1,0,2)};
   gate=data.settings.voiceOnly?createVoiceGate():null;dry=new Float32Array(960);postMessage({type:'ready'});return;
  }
  if(data.type==='gain'){settings.gain=bounded(data.value,1,0,2);return;}
  if(data.type==='typing'){typing=20;return;}
  if(data.type!=='stream')return;
  if(started||!model)throw Error('Invalid stream lifecycle');started=true;
  const reader=data.readable.getReader(),writer=data.writable.getWriter(),input=new Float32Array(480);
  let fill=0,timestamp=0,frames=0,sourceSamples=0,renderedSamples=0,sourceStart,maxCost=0,writeMax=0,initialRebases=0,discardedPrimingSamples=0;
  try{for(;;){const{value:frame,done}=await reader.read();if(done)throw Error('Capture stream ended');try{
   if(frame.sampleRate!==48000||frame.numberOfChannels<1||frame.numberOfChannels>8||frame.numberOfFrames>4800)throw Error('Unsupported capture format');
   if(sourceStart===undefined)sourceStart=frame.timestamp;
   // Do not relabel dropped capture time as continuous low-latency audio.
   const gap=frame.timestamp-sourceStart-sourceSamples/48000*1e6;
   if(Math.abs(gap)>20000){
    // Some capture devices replace the startup epoch after their first frame.
    // No priming audio is sent; only this pre-send clock may be rebased.
    if(frames<100&&renderedSamples===0&&initialRebases<3){sourceStart+=gap;initialRebases++;discardedPrimingSamples+=fill;fill=0;}
    else throw Error('Capture discontinuity: '+Math.round(gap)+'us at frame '+frames);
   }
   const mono=new Float32Array(frame.numberOfFrames),channel=new Float32Array(frame.numberOfFrames);
   for(let c=0;c<frame.numberOfChannels;c++){frame.copyTo(channel,{planeIndex:c,format:'f32-planar'});for(let i=0;i<mono.length;i++)mono[i]+=channel[i]/frame.numberOfChannels;}
   sourceSamples+=mono.length;
   for(let at=0;at<mono.length;){if(!fill)timestamp=frame.timestamp+at/48000*1e6;const count=Math.min(480-fill,mono.length-at);input.set(mono.subarray(at,at+count),fill);fill+=count;at+=count;if(fill<480)continue;fill=0;
    if(!input.every(Number.isFinite))throw Error('Nonfinite input');
    const start=performance.now(),output=render(input);maxCost=Math.max(maxCost,performance.now()-start);frames++;
    // Prime against the live microphone before accepting speech; never queue
    // this startup history for subsequent playback.
    if(frames<=100){if(frames===100)postMessage({type:'primed',speaking,preparing});continue;}
    const audio=new AudioData({format:'f32-planar',sampleRate:48000,numberOfFrames:480,numberOfChannels:1,timestamp:Math.round(timestamp),data:output});
    const writeAt=performance.now();try{await writer.write(audio);}finally{audio.close();}writeMax=Math.max(writeMax,performance.now()-writeAt);renderedSamples+=480;
    if(writeMax>100)throw Error('Capture output backlog');
    if(frames%100===0)postMessage({type:'stats',frames,sourceSamples,renderedSamples,primingSamples:48000,maxCost,writeMax,initialRebases,discardedPrimingSamples,pendingSamples:mono.length-at+fill});
   }
  }finally{frame.close();}}}finally{await reader.cancel().catch(()=>{});await writer.close().catch(()=>{});model.destroy();model=undefined;}
 }catch(error){postMessage({type:'error',reason:error.message});}
};
