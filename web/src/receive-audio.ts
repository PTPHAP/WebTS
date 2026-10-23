// Automatic level adjustment is for remote speakers, separate from microphone AGC.
export function receiveGain(rms:number,current:number,enabled:boolean):number {
  if(!enabled)return 1;
  if(!Number.isFinite(rms))return 1;
  if(rms<.008)return current+(1-current)*.15; // Release boost during pauses instead of amplifying hiss.
  const target=Math.max(.5,Math.min(4,.12/rms));
  return current+(target-current)*(target<current?.35:.08);
}
export function createReceiver(context:AudioContext,track:MediaStreamTrack,rtpReceiver?:RTCRtpReceiver) {
  if(rtpReceiver){
    const hints=rtpReceiver as RTCRtpReceiver & {jitterBufferTarget?:number|null;playoutDelayHint?:number|null};
    try{if('jitterBufferTarget' in hints)hints.jitterBufferTarget=0;}catch{}
    try{if('playoutDelayHint' in hints)hints.playoutDelayHint=0;}catch{}
  }
  const direct=typeof (context as AudioContext & {setSinkId?:unknown}).setSinkId==='function';
  // Chromium can leave remote RTC decoding idle when only Web Audio consumes it.
  // Never route original PCM through the helper: element volume is not a silence guarantee.
  const silent=track.clone();silent.enabled=false;
  const decoder=new Audio();decoder.muted=true;decoder.volume=0;decoder.srcObject=new MediaStream([silent]);
  const source=context.createMediaStreamSource(new MediaStream([track])),analyser=context.createAnalyser(),gain=context.createGain(),limiter=context.createDynamicsCompressor(),volume=context.createGain(),destination=context.createMediaStreamDestination();
  analyser.fftSize=512;limiter.threshold.value=-1;limiter.knee.value=0;limiter.ratio.value=20;limiter.attack.value=.003;limiter.release.value=.08;
  source.connect(analyser);source.connect(gain);gain.connect(limiter);limiter.connect(volume);
  // Bypass the stream-to-element playback queue when the context can also select its output device.
  if(direct)volume.connect(context.destination);else volume.connect(destination);
  const data=new Float32Array(analyser.fftSize);let current=1;
  return {direct,stream:destination.stream,play(){return decoder.play();},volume(value:number){volume.gain.value=Number.isFinite(value)?Math.max(0,Math.min(1,value)):0;},tick(enabled:boolean){analyser.getFloatTimeDomainData(data);const rms=Math.sqrt(data.reduce((s,n)=>s+n*n,0)/data.length);current=receiveGain(rms,current,enabled);gain.gain.setTargetAtTime(current,context.currentTime,.03);},destroy(){decoder.pause();decoder.srcObject=null;silent.stop();for(const node of [source,analyser,gain,limiter,volume,destination])node.disconnect();destination.stream.getTracks().forEach(t=>t.stop());}};
}
