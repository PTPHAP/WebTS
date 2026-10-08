// Automatic level adjustment is for remote speakers, separate from microphone AGC.
export function receiveGain(rms:number,current:number,enabled:boolean):number {
  if(!enabled)return 1;
  if(!Number.isFinite(rms))return 1;
  if(rms<.008)return current+(1-current)*.15; // Release boost during pauses instead of amplifying hiss.
  const target=Math.max(.5,Math.min(4,.12/rms));
  return current+(target-current)*(target<current?.35:.08);
}
export function createReceiver(context:AudioContext,track:MediaStreamTrack) {
  // Chromium can leave remote RTC decoding idle when only Web Audio consumes it.
  // Start the original track silently; the processed stream is the audible output.
  const decoder=new Audio();decoder.srcObject=new MediaStream([track]);decoder.volume=0;
  const source=context.createMediaStreamSource(new MediaStream([track])),analyser=context.createAnalyser(),gain=context.createGain(),limiter=context.createDynamicsCompressor(),destination=context.createMediaStreamDestination();
  analyser.fftSize=512;limiter.threshold.value=-1;limiter.knee.value=0;limiter.ratio.value=20;limiter.attack.value=.003;limiter.release.value=.08;
  source.connect(analyser);source.connect(gain);gain.connect(limiter);limiter.connect(destination);
  const data=new Float32Array(analyser.fftSize);let current=1;
  return {stream:destination.stream,play(){return decoder.play();},tick(enabled:boolean){analyser.getFloatTimeDomainData(data);const rms=Math.sqrt(data.reduce((s,n)=>s+n*n,0)/data.length);current=receiveGain(rms,current,enabled);gain.gain.setTargetAtTime(current,context.currentTime,.03);},destroy(){decoder.pause();decoder.srcObject=null;for(const node of [source,analyser,gain,limiter,destination])node.disconnect();destination.stream.getTracks().forEach(t=>t.stop());}};
}
