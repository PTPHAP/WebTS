import {test,expect} from '@playwright/test';

test('playback peak protection adds no lookahead and preserves ordinary speech samples',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);
 const result=await page.evaluate(async()=>{
  const {createPlaybackLimiter}=window.audioFixture;
  const context=new OfflineAudioContext(1,8192,48000),source=context.createBufferSource(),buffer=context.createBuffer(1,8192,48000),input=buffer.getChannelData(0);
  input[512]=.2;for(let i=1024;i<4096;i++)input[i]=.6*Math.sin(i*.37);for(let i=4608;i<7680;i++)input[i]=4*Math.sin(i*.29);
  source.buffer=buffer;const limiter=createPlaybackLimiter(context);source.connect(limiter);limiter.connect(context.destination);source.start();
  const output=(await context.startRendering()).getChannelData(0);let error=0,peak=0,first=-1;
  for(let i=0;i<output.length;i++){if(!Number.isFinite(output[i]))throw Error('Nonfinite protected output');if(first<0&&Math.abs(output[i])>1e-5)first=i;if(i>=1024&&i<4096)error=Math.max(error,Math.abs(input[i]-output[i]));peak=Math.max(peak,Math.abs(output[i]));}
  return{first,error,peak};
 });
 expect(result.first).toBe(512);expect(result.error).toBeLessThan(1e-6);expect(result.peak).toBeLessThanOrEqual(1);expect(result.peak).toBeGreaterThan(.8);
});

test('a real audio context rejects a removed sink without blocking voice reconnect',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);await page.locator('body').click();
 test.skip(!await page.evaluate(()=>typeof AudioContext.prototype.setSinkId==='function'),'Context output selection is unavailable in this browser.');
 const result=await page.evaluate(async()=>{
  const {Voice}=window.audioFixture,original={fetch:window.fetch,WebSocket:window.WebSocket},sockets=[],events=[];
  let voice;
  try{
   window.fetch=(url,options)=>String(url).split('?')[0]==='/api/rtc'?Promise.resolve(new Response('{"iceServers":[]}')):original.fetch(url,options);
   window.WebSocket=class{static OPEN=1;readyState=1;constructor(){sockets.push(this);queueMicrotask(()=>this.onopen?.());}send(){}close(){this.readyState=3;}};
   voice=new Voice(e=>events.push(e));await voice.connect({identity:'synthetic-only'},'',true);
   voice.sink='webts-nonexistent-unplugged-headset';sockets[0].onclose();
   const deadline=Date.now()+7000;while(sockets.length<2&&Date.now()<deadline)await new Promise(r=>setTimeout(r,20));
   return {sockets:sockets.length,open:sockets.at(-1).readyState===1,peer:!!voice.peer,sink:voice.sink,actualSink:voice.context?.sinkId,notice:events.some(e=>e.type==='notice'&&/默认.*输出/.test(e.message)),errors:events.filter(e=>e.type==='error')};
  }finally{voice?.close();window.fetch=original.fetch;window.WebSocket=original.WebSocket;}
 });
 expect(result).toEqual({sockets:2,open:true,peer:true,sink:'',actualSink:'',notice:true,errors:[]});
});

test('cached voice6 clients can initialize the original worklet after a clean deployment',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);await page.locator('body').click();
 const result=await page.evaluate(async()=>{
  const context=new AudioContext({sampleRate:48000});let node;
  try{
   const response=await fetch('/audio/0.4.1-rnnoise.wasm');if(!response.ok)throw Error('Legacy WASM unavailable');
   const wasmBinary=await response.arrayBuffer();await context.audioWorklet.addModule('/audio/rnnoise-0.4.1-voice6.js');
   node=new AudioWorkletNode(context,'@sapphi-red/web-noise-suppressor/rnnoise',{outputChannelCount:[1],processorOptions:{maxChannels:1,wasmBinary,strength:1}});
   return await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Legacy ready timeout')),5000);node.port.onmessage=e=>{clearTimeout(timer);resolve(e.data.type);};node.onprocessorerror=()=>{clearTimeout(timer);reject(Error('Legacy processor error'));};});
  }finally{node?.port.postMessage('destroy');node?.disconnect();await context.close();}
 });
 expect(result).toBe('ready');
});

test('real AI fallback remains silent until the worklet acknowledges filtering and then resumes audio',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);await page.locator('body').click();
 test.skip(!await page.evaluate(()=>typeof AudioContext==='function'),'AudioContext unavailable; physical phone validation remains separate.');
 const result=await page.evaluate(async()=>{
  const {createNoise}=window.audioFixture,Original=window.AudioWorkletNode,worklets=[];
  const context=new AudioContext({sampleRate:48000,latencyHint:'interactive'}),source=context.createBufferSource(),level=context.createGain(),analyser=context.createAnalyser(),silent=context.createGain();silent.gain.value=0;level.gain.value=1;
  const buffer=context.createBuffer(1,48000*4,48000),samples=buffer.getChannelData(0);let phase=0;for(let i=0;i<samples.length;i++){const t=i/48000;phase+=2*Math.PI*(150+30*Math.sin(t*12))/48000;for(let h=1;h<16;h++)samples[i]+=Math.sin(phase*h)/h*.015*(.3+.7*Math.sin(t*9)**2);}source.buffer=buffer;source.loop=true;
  let processor,notice=false,error=false,started=false;
  window.AudioWorkletNode=class extends Original{constructor(c,id,options){super(c,id,options);worklets.push({id,node:this});}};
  const pause=ms=>new Promise(r=>setTimeout(r,ms)),read=async()=>{const end=context.currentTime+.25,deadline=Date.now()+3000;while(context.currentTime<end){if(Date.now()>deadline)throw Error('audio clock stopped');await pause(20);}const samples=new Float32Array(2048);analyser.getFloatTimeDomainData(samples);return Math.sqrt(samples.reduce((n,x)=>n+x*x,0)/samples.length);};
  try{
   processor=await createNoise(context,true,false);if(processor.mode!=='keyboard')throw Error('GTCRN did not initialize');
   processor.onchange=()=>notice=true;processor.onerror=()=>error=true;source.connect(level);level.connect(processor.input);processor.output.connect(analyser);analyser.connect(silent);silent.connect(context.destination);source.start();started=true;await context.resume();const before=await read();
   const originalOutput=processor.output,keyboard=worklets.find(w=>w.id.endsWith('/gtcrn')).node;
   keyboard.onprocessorerror(new Event('processorerror'));
   const immediatelyMuted=processor.output.gain.value===0;
   const deadline=Date.now()+2500;while(!notice&&!error&&Date.now()<deadline)await pause(10);
   return {before,immediatelyMuted,acknowledged:notice,error,mode:processor.mode,sameOutput:processor.output===originalOutput,gain:processor.output.gain.value,rms:await read()};
  }finally{window.AudioWorkletNode=Original;processor?.destroy();if(started)source.stop();for(const node of [source,level,analyser,silent])node.disconnect();await context.close();}
 });
 expect(result.immediatelyMuted,JSON.stringify(result)).toBe(true);expect(result.acknowledged).toBe(true);expect(result.error).toBe(false);expect(result.mode).toBe('rnnoise');expect(result.sameOutput).toBe(true);expect(result.gain).toBe(1);expect(result.before,JSON.stringify(result)).toBeGreaterThan(.0001);expect(result.rms,JSON.stringify(result)).toBeGreaterThan(.0001);
});

test('real RTC speech control bypasses delayed signalling and falls back when its channel closes',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);await page.locator('body').click();
 test.skip(!await page.evaluate(()=>typeof AudioContext==='function'),'This local browser build does not provide AudioContext; physical mobile validation remains required.');
 const result=await page.evaluate(async()=>{
  const {Voice,defaultAudioSettings}=window.audioFixture,pause=ms=>new Promise(r=>setTimeout(r,ms));
  const context=new AudioContext(),source=context.createOscillator(),input=context.createMediaStreamDestination(),remote=new RTCPeerConnection({iceServers:[]});
  const original={fetch:window.fetch,WebSocket:window.WebSocket,getUserMedia:navigator.mediaDevices.getUserMedia};
  let voice,socket,answer,error;const pending=[],direct=[],signalled=[],events=[];
  const deliver=m=>socket.onmessage?.({data:JSON.stringify(m)});
  const wait=async(check)=>{const end=Date.now()+8000;while(!check()&&Date.now()<end){if(error)throw Error(error);await pause(20);}if(!check())throw Error('RTC control timed out');};
  const channel=remote.createDataChannel('webts-audio-control-v1');
  channel.onopen=()=>channel.send('{"type":"audio_control_ready"}');channel.onmessage=e=>direct.push(JSON.parse(e.data));
  try{
   source.connect(input);source.start();await context.resume();navigator.mediaDevices.getUserMedia=async()=>input.stream.clone();
   window.fetch=(url,options)=>String(url).split('?')[0]==='/api/rtc'?Promise.resolve(new Response('{"iceServers":[]}')):original.fetch(url,options);
   // Negotiation is real; transmit signalling is deliberately never delivered to the peer.
   window.WebSocket=class{static OPEN=1;readyState=1;constructor(){socket=this;queueMicrotask(()=>this.onopen?.());}send(raw){const m=JSON.parse(raw);if(m.type==='transmit')signalled.push(m);if(m.type==='answer'){answer=remote.setRemoteDescription(m.description).then(async()=>{for(const c of pending)await remote.addIceCandidate(c);});answer.catch(e=>error=e.message);}if(m.type==='ice'){if(answer)answer.then(()=>remote.addIceCandidate(m.candidate)).catch(e=>error=e.message);else pending.push(m.candidate);}}close(){this.readyState=3;}};
   remote.onicecandidate=e=>{if(e.candidate)deliver({type:'ice',candidate:e.candidate.toJSON()});};remote.addTransceiver('audio',{direction:'recvonly'});
   voice=new Voice(e=>events.push(e));await voice.configure({...defaultAudioSettings,noise:'off',volume:0});await voice.connect({identity:'synthetic-only'},'',false);
   deliver({type:'state',own:1,members:[{id:1,channel:1}],channels:[{id:1}]});const offer=await remote.createOffer();await remote.setLocalDescription(offer);deliver({type:'offer',description:offer});
   await wait(()=>events.some(e=>e.type==='audio_control'&&e.state==='direct'));await wait(()=>direct.length>0);
   const start=direct.length,wsStart=signalled.length;voice.setMute(true,false);await wait(()=>direct.length>start);voice.setMute(false,false);await wait(()=>direct.length>start+1);
   const toggles=direct.slice(start),bypassed=signalled.length===wsStart;
   channel.close();await wait(()=>events.some(e=>e.type==='audio_control'&&e.state==='signalling'));await wait(()=>signalled.length>wsStart);
   const fallback=signalled.at(-1);return {toggles,bypassed,fallback};
  }finally{voice?.close();remote.close();source.stop();input.stream.getTracks().forEach(t=>t.stop());await context.close();window.fetch=original.fetch;window.WebSocket=original.WebSocket;navigator.mediaDevices.getUserMedia=original.getUserMedia;}
 });
 expect(result.bypassed).toBe(true);expect(result.toggles.map(m=>m.enabled)).toEqual([false,true]);expect(result.toggles.every(m=>m.type==='transmit')).toBe(true);
 expect(result.fallback.enabled).toBe(true);expect(result.fallback.sequence).toBeGreaterThan(result.toggles.at(-1).sequence);
});

test('real RTC renegotiation reuses playback and remains bidirectional through mute and recovery',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);
 test.skip(!await page.evaluate(()=>typeof AudioContext==='function'),'This local browser build does not provide AudioContext; not a physical mobile acceptance.');
 await page.locator('body').click();
 const result=await page.evaluate(async()=>{
  const {Voice,defaultAudioSettings}=window.audioFixture,pause=ms=>new Promise(r=>setTimeout(r,ms));
  const context=new AudioContext(),source=context.createOscillator(),input=context.createMediaStreamDestination(),remote=new RTCPeerConnection({iceServers:[]});
  const microphone=context.createMediaStreamDestination(),microphoneGain=context.createGain();source.connect(microphoneGain);microphoneGain.connect(microphone);
  const original={fetch:window.fetch,WebSocket:window.WebSocket,getUserMedia:navigator.mediaDevices.getUserMedia,Audio:window.Audio,connect:AudioNode.prototype.connect},players=[],connections=[];
  let voice,socket,remoteMeter,remotePlayback,answer,signalingError;const pending=[],errors=[];
  const deliver=m=>socket.onmessage?.({data:JSON.stringify(m)});
  const rms=analyser=>{const data=new Float32Array(analyser.fftSize);analyser.getFloatTimeDomainData(data);return Math.sqrt(data.reduce((s,n)=>s+n*n,0)/data.length);};
  try{
   window.Audio=function(...args){const audio=new original.Audio(...args);players.push(audio);return audio;};
   AudioNode.prototype.connect=function(target,...args){connections.push([this,target]);return original.connect.call(this,target,...args);};
   source.frequency.value=440;source.connect(input);source.start();await context.resume();
   navigator.mediaDevices.getUserMedia=async()=>microphone.stream.clone();window.fetch=(url,options)=>String(url).split('?')[0]==='/api/rtc'?Promise.resolve(new Response('{"iceServers":[]}')):original.fetch(url,options);
   window.WebSocket=class{static OPEN=1;readyState=1;constructor(){socket=this;queueMicrotask(()=>this.onopen?.());}send(raw){const m=JSON.parse(raw);if(m.type==='answer'){answer=remote.setRemoteDescription(m.description).then(async()=>{for(const c of pending)await remote.addIceCandidate(c);});answer.catch(e=>signalingError=e.message);}if(m.type==='ice'){if(answer)answer.then(()=>remote.addIceCandidate(m.candidate)).catch(e=>signalingError=e.message);else pending.push(m.candidate);}}close(){this.readyState=3;}};
   remote.onicecandidate=e=>{if(e.candidate)deliver({type:'ice',candidate:e.candidate.toJSON()});};
   remote.ontrack=e=>{const node=context.createMediaStreamSource(new MediaStream([e.track]));remoteMeter=context.createAnalyser();node.connect(remoteMeter);remotePlayback=new Audio();remotePlayback.volume=0;remotePlayback.srcObject=new MediaStream([e.track]);remotePlayback.play().catch(e=>errors.push(e.message));};
   remote.addTrack(input.stream.getAudioTracks()[0],input.stream);voice=new Voice(e=>{if(e.type==='error')errors.push(String(e.message));});await voice.configure({...defaultAudioSettings,noise:'off',volume:0,ducking:0,receiveAutoGain:false});await voice.connect({identity:'synthetic-only'},'',false);
   deliver({type:'state',own:1,members:[{id:1,channel:1}],channels:[{id:1}]});const offer=await remote.createOffer();await remote.setLocalDescription(offer);deliver({type:'offer',description:offer});
   const end=Date.now()+8000;while(Date.now()<end&&(voice.peer.connectionState!=='connected'||!voice.receivers.size||!remoteMeter||rms(remoteMeter)<.001))await pause(50);
   if(signalingError)throw Error(signalingError);if(!voice.receivers.size||!remoteMeter)throw Error('loopback did not receive audio');
   const receiver=[...voice.receivers.values()][0],player=[...voice.elements.values()][0],rtpReceiver=voice.peer.getReceivers()[0],bufferHints={jitterSupported:'jitterBufferTarget'in rtpReceiver,playoutSupported:'playoutDelayHint'in rtpReceiver,jitter:rtpReceiver.jitterBufferTarget,playout:rtpReceiver.playoutDelayHint},meter=voice.context.createAnalyser(),expectedDirect=typeof voice.context.setSinkId==='function',playerMutedBefore=player.muted;
   const streamMeter=voice.context.createAnalyser();voice.context.createMediaStreamSource(receiver.stream).connect(streamMeter);
   if(expectedDirect){const output=connections.find(([,target])=>target===voice.context.destination)?.[0];if(!output)throw Error('No direct playback route');output.connect(meter);}else voice.context.createMediaStreamSource(receiver.stream).connect(meter);
   const helper=players.find(audio=>audio.srcObject?.getAudioTracks()[0]?.enabled===false);if(!helper)throw Error('No disabled helper clone');
   helper.muted=false;helper.volume=1;await helper.play(); // Deliberately defeat both element-level safeguards.
   const helperMeter=voice.context.createAnalyser(),helperSource=voice.context.createMediaStreamSource(helper.srcObject),zero=voice.context.createGain();zero.gain.value=0;helperSource.connect(helperMeter);helperMeter.connect(zero);zero.connect(voice.context.destination);
   await voice.handling;await answer;let repeatedEvents=0;const ontrack=voice.peer.ontrack;
   voice.peer.ontrack=event=>{repeatedEvents++;ontrack(event);};
   remote.getSenders()[0].setStreams(new MediaStream([input.stream.getAudioTracks()[0]]));
   const revised=await remote.createOffer();await remote.setLocalDescription(revised);deliver({type:'offer',description:revised});await voice.handling;await answer;
   const reusedPlayer=[...voice.elements.values()][0]===player&&[...voice.receivers.values()][0]===receiver;
   const read=async()=>{await pause(200);return rms(meter);};
   voice.outputVolume(.25);for(const audio of voice.elements.values())audio.muted=true;const heard=await read(),sent=rms(remoteMeter);
   voice.setMute(false,true);const muted=await read();voice.setMute(false,false);const playerMutedAfterRestore=player.muted;for(const audio of voice.elements.values())audio.muted=true;const restored=await read();
   const helperRms=rms(helperMeter),unusedStreamRms=rms(streamMeter);helperSource.disconnect();helperMeter.disconnect();zero.disconnect();
   await voice.configure({...voice.settings,ducking:1});microphoneGain.gain.value=0;await pause(500);const idleWithoutNoise=rms(meter),stillTransmitting=voice.active;
   microphoneGain.gain.value=.1;await pause(400);const duckedDuringSpeech=rms(meter);
   microphoneGain.gain.value=0;await pause(500);const afterSpeech=rms(meter);
   const state=voice.peer.connectionState,count=voice.elements.size,helperTrack=helper.srcObject.getAudioTracks()[0],snapshot=voice.diagnostics();voice.close();const closedSnapshot=voice.diagnostics();voice=undefined;
   return {state,snapshot,closedSnapshot,repeatedEvents,reusedPlayer,players:count,heard,sent,muted,restored,idleWithoutNoise,stillTransmitting,duckedDuringSpeech,afterSpeech,helperRms,unusedStreamRms,helperReleased:helperTrack.readyState==='ended',errors,direct:receiver.direct,expectedDirect,playerMutedBefore,playerMutedAfterRestore,bufferHints};
  }finally{voice?.close();remotePlayback?.pause();if(remotePlayback)remotePlayback.srcObject=null;remote.close();source.stop();input.stream.getTracks().forEach(t=>t.stop());await context.close();window.fetch=original.fetch;window.WebSocket=original.WebSocket;window.Audio=original.Audio;AudioNode.prototype.connect=original.connect;navigator.mediaDevices.getUserMedia=original.getUserMedia;}
 });
 expect(result.snapshot).toEqual({context:'running',processing:'off',receivers:1,players:1,pausedPlayers:0});expect(result.closedSnapshot).toEqual({context:'inactive',processing:'inactive',receivers:0,players:0,pausedPlayers:0});expect(result.errors).toEqual([]);expect(result.state).toBe('connected');expect(result.repeatedEvents).toBe(1);expect(result.reusedPlayer).toBe(true);expect(result.players).toBe(1);expect(result.heard).toBeGreaterThan(.001);expect(result.sent).toBeGreaterThan(.001);expect(result.muted).toBeLessThan(.00001);expect(result.restored).toBeGreaterThan(.001);expect(result.helperRms).toBe(0);expect(result.helperReleased).toBe(true);expect(result.direct).toBe(result.expectedDirect);expect(result.playerMutedBefore).toBe(result.expectedDirect);expect(result.playerMutedAfterRestore).toBe(result.expectedDirect);
 expect(result.stillTransmitting).toBe(true);expect(result.idleWithoutNoise).toBeGreaterThan(.001);expect(result.duckedDuringSpeech).toBeLessThan(.00001);expect(result.afterSpeech).toBeGreaterThan(.001);
 expect(result.bufferHints.jitterSupported).toBe(true);expect(result.bufferHints.jitter).toBe(0);if(result.bufferHints.playoutSupported)expect(result.bufferHints.playout).toBe(0);
 if(result.direct)expect(result.unusedStreamRms).toBe(0);
});

test('real audio graph retains output attenuation and mute when element volume is ignored',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);
 test.skip(!await page.evaluate(()=>typeof AudioContext==='function'),'This local browser build does not provide AudioContext; physical mobile validation remains required.');
 await page.locator('body').click();
 const result=await page.evaluate(async()=>{
  const {createReceiver}=window.audioFixture,OriginalAudio=window.Audio,audios=[];
  // Reproduce only the ignored element-volume behavior; the graph below is real Web Audio.
  window.Audio=class{constructor(){audios.push(this);}get volume(){return 1;}set volume(_){}get muted(){return false;}set muted(_){}play(){this.plays=(this.plays??0)+1;return Promise.resolve();}pause(){}};
  const context=new AudioContext(),source=context.createOscillator(),input=context.createMediaStreamDestination(),silent=context.createGain();silent.gain.value=0;Object.defineProperty(context,'setSinkId',{value:undefined}); // Exercise element playback as on browsers without context sink selection.
  let receiver,meterSource;const analyser=context.createAnalyser();analyser.fftSize=2048;
  const read=async()=>{const rendered=context.currentTime+.16,deadline=Date.now()+3000;while(context.currentTime<rendered&&Date.now()<deadline)await new Promise(r=>setTimeout(r,20));if(context.currentTime<rendered)throw Error('Audio render clock did not advance');const data=new Float32Array(analyser.fftSize);analyser.getFloatTimeDomainData(data);return Math.sqrt(data.reduce((s,n)=>s+n*n,0)/data.length);};
  try{
   source.frequency.value=440;source.connect(input);source.start();await context.resume();receiver=createReceiver(context,input.stream.getAudioTracks()[0]);await receiver.play();
   meterSource=context.createMediaStreamSource(receiver.stream);meterSource.connect(analyser);analyser.connect(silent);silent.connect(context.destination);
   receiver.volume(.5);const half=await read();receiver.volume(.25);const quarter=await read();receiver.tick(true);const agc=await read();receiver.volume(0);const muted=await read();receiver.volume(.5);const restored=await read();
   return {half,quarter,agc,muted,restored,helperCopies:audios.filter(a=>a.srcObject&&a.plays&&a.volume!==0&&!a.muted&&a.srcObject.getAudioTracks()[0]?.enabled!==false).length};
  }finally{window.Audio=OriginalAudio;receiver?.destroy();meterSource?.disconnect();analyser.disconnect();silent.disconnect();source.stop();source.disconnect();input.stream.getTracks().forEach(t=>t.stop());input.disconnect();await context.close();}
 });
 expect(result.helperCopies).toBe(0);expect(result.half).toBeGreaterThan(.01);expect(result.quarter/result.half).toBeGreaterThan(.45);expect(result.quarter/result.half).toBeLessThan(.55);expect(result.agc).toBeGreaterThan(.01);expect(result.muted).toBeLessThan(.00001);expect(result.restored).toBeGreaterThan(.01);
});
