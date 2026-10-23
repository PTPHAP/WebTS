import type {NoiseProcessor} from './noise';
type TrackConstructor=new(options:{kind:string})=>MediaStreamTrack&{writable:WritableStream};
type ProcessorConstructor=new(options:{track:MediaStreamTrack})=>{readable:ReadableStream};
const constructors=()=>globalThis as typeof globalThis&{MediaStreamTrackGenerator?:TrackConstructor;MediaStreamTrackProcessor?:ProcessorConstructor};
export function supportsStreamNoise(){const api=constructors();return typeof Worker==='function'&&!!api.MediaStreamTrackGenerator&&!!api.MediaStreamTrackProcessor;}
export async function createStreamNoise(context:AudioContext,stream:MediaStream,voiceOnly:boolean,strength:number,gain:number):Promise<NoiseProcessor>{
 const api=constructors();if(!supportsStreamNoise()||stream.getAudioTracks()[0]?.getSettings().noiseSuppression!==true)throw Error('Realtime processing unavailable');
 const input=stream.getAudioTracks()[0].clone(),placeholder=context.createGain();let worker:Worker|undefined,output:(MediaStreamTrack&{writable:WritableStream})|undefined,closed=false;
 let rejectPending:((error:Error)=>void)|undefined,timer:ReturnType<typeof setTimeout>|undefined;
 const destroy=()=>{if(closed)return;closed=true;clearTimeout(timer);rejectPending?.(Error('Processing cancelled'));rejectPending=undefined;input.stop();output?.stop();worker?.terminate();placeholder.disconnect();};
 try{
  worker=new Worker('/audio/realtime-noise-v1.js',{type:'module'});
  await new Promise<void>((resolve,reject)=>{rejectPending=reject;timer=setTimeout(()=>reject(Error('Processing initialization timeout')),8000);worker!.onerror=()=>reject(Error('Processing initialization failed'));worker!.onmessage=({data})=>{if(data.type==='ready')resolve();else if(data.type==='error')reject(Error('Processing initialization failed'));};worker!.postMessage({type:'init',settings:{voiceOnly,strength,gain}});});
  clearTimeout(timer);rejectPending=undefined;
  output=new api.MediaStreamTrackGenerator!({kind:'audio'});const source=new api.MediaStreamTrackProcessor!({track:input});
  const node:NoiseProcessor={input:placeholder,output:placeholder,stream:new MediaStream([output]),mode:'realtime',speaking:false,preparing:false,gain(value){if(!closed)worker!.postMessage({type:'gain',value});},typing(){if(!closed)worker!.postMessage({type:'typing'});},destroy};
  await new Promise<void>((resolve,reject)=>{
   rejectPending=reject;timer=setTimeout(()=>reject(Error('Capture priming timeout')),5000);
   worker!.onerror=()=>{node.failure='Processing worker failed';if(rejectPending)reject(Error('Processing worker failed'));else{destroy();node.onerror?.();}};
   worker!.onmessage=({data})=>{if(closed)return;if(data.type==='primed'){clearTimeout(timer);rejectPending=undefined;node.speaking=data.speaking===true;node.preparing=data.preparing===true;resolve();}else if(data.type==='speech'){node.speaking=data.enabled===true;node.onspeech?.(node.speaking);}else if(data.type==='prepare'){node.preparing=data.enabled===true;node.onprepare?.(node.preparing);}else if(data.type==='stats'){const {frames,sourceSamples,renderedSamples,primingSamples,maxCost,writeMax,initialRebases,discardedPrimingSamples,pendingSamples}=data;node.diagnostics={frames,sourceSamples,renderedSamples,primingSamples,maxCost,writeMax,initialRebases,discardedPrimingSamples,pendingSamples};}else if(data.type==='error'){node.failure=typeof data.reason==='string'?data.reason.slice(0,160):'Capture processing failed';if(rejectPending)reject(Error('Capture processing failed'));else{destroy();node.onerror?.();}}};
   worker!.postMessage({type:'stream',readable:source.readable,writable:output!.writable},[source.readable,output!.writable]);
  });
  return node;
 }catch(error){destroy();throw error;}
}
