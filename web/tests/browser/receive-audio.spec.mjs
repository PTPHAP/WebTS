import {test,expect} from '@playwright/test';

test('real AI fallback remains silent until the worklet acknowledges filtering and then resumes audio',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);await page.locator('body').click();
 test.skip(!await page.evaluate(()=>typeof AudioContext==='function'),'AudioContext unavailable; physical phone validation remains separate.');
 const result=await page.evaluate(async()=>{
  const {createNoise}=window.audioFixture,Original=window.AudioWorkletNode,worklets=[];
  const context=new AudioContext({sampleRate:48000,latencyHint:'interactive'}),source=context.createBufferSource(),level=context.createGain(),analyser=context.createAnalyser(),silent=context.createGain();silent.gain.value=0;level.gain.value=1;
  const buffer=context.createBuffer(1,48000*4,48000),samples=buffer.getChannelData(0);let phase=0;for(let i=0;i<samples.length;i++){const t=i/48000;phase+=2*Math.PI*(150+30*Math.sin(t*12))/48000;for(let h=1;h<16;h++)samples[i]+=Math.sin(phase*h)/h*.015*(.3+.7*Math.sin(t*9)**2);}source.buffer=buffer;source.loop=true;
  let processor,notice=false,error=false;
  window.AudioWorkletNode=class extends Original{constructor(c,id,options){super(c,id,options);worklets.push({id,node:this});}};
  const pause=ms=>new Promise(r=>setTimeout(r,ms)),read=async()=>{const end=context.currentTime+.25,deadline=Date.now()+3000;while(context.currentTime<end){if(Date.now()>deadline)throw Error('audio clock stopped');await pause(20);}const samples=new Float32Array(2048);analyser.getFloatTimeDomainData(samples);return Math.sqrt(samples.reduce((n,x)=>n+x*x,0)/samples.length);};
  try{
   processor=await createNoise(context,true,false);if(processor.mode!=='keyboard')throw Error('GTCRN did not initialize');
   processor.onchange=()=>notice=true;processor.onerror=()=>error=true;source.connect(level);level.connect(processor.input);processor.output.connect(analyser);analyser.connect(silent);silent.connect(context.destination);source.start();await context.resume();const before=await read();
   const originalOutput=processor.output,keyboard=worklets.find(w=>w.id.endsWith('/gtcrn')).node;
   keyboard.onprocessorerror(new Event('processorerror'));
   const immediatelyMuted=processor.output.gain.value===0;
   const deadline=Date.now()+2500;while(!notice&&!error&&Date.now()<deadline)await pause(10);
   return {before,immediatelyMuted,acknowledged:notice,error,mode:processor.mode,sameOutput:processor.output===originalOutput,gain:processor.output.gain.value,rms:await read()};
  }finally{window.AudioWorkletNode=Original;processor?.destroy();source.stop();for(const node of [source,level,analyser,silent])node.disconnect();await context.close();}
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
  const original={fetch:window.fetch,WebSocket:window.WebSocket,getUserMedia:navigator.mediaDevices.getUserMedia,Audio:window.Audio},players=[];
  let voice,socket,remoteMeter,remotePlayback,answer,signalingError;const pending=[],errors=[];
  const deliver=m=>socket.onmessage?.({data:JSON.stringify(m)});
  const rms=analyser=>{const data=new Float32Array(analyser.fftSize);analyser.getFloatTimeDomainData(data);return Math.sqrt(data.reduce((s,n)=>s+n*n,0)/data.length);};
  try{
   window.Audio=function(...args){const audio=new original.Audio(...args);players.push(audio);return audio;};
   source.frequency.value=440;source.connect(input);source.start();await context.resume();
   navigator.mediaDevices.getUserMedia=async()=>input.stream.clone();window.fetch=(url,options)=>String(url).split('?')[0]==='/api/rtc'?Promise.resolve(new Response('{"iceServers":[]}')):original.fetch(url,options);
   window.WebSocket=class{static OPEN=1;readyState=1;constructor(){socket=this;queueMicrotask(()=>this.onopen?.());}send(raw){const m=JSON.parse(raw);if(m.type==='answer'){answer=remote.setRemoteDescription(m.description).then(async()=>{for(const c of pending)await remote.addIceCandidate(c);});answer.catch(e=>signalingError=e.message);}if(m.type==='ice'){if(answer)answer.then(()=>remote.addIceCandidate(m.candidate)).catch(e=>signalingError=e.message);else pending.push(m.candidate);}}close(){this.readyState=3;}};
   remote.onicecandidate=e=>{if(e.candidate)deliver({type:'ice',candidate:e.candidate.toJSON()});};
   remote.ontrack=e=>{const node=context.createMediaStreamSource(new MediaStream([e.track]));remoteMeter=context.createAnalyser();node.connect(remoteMeter);remotePlayback=new Audio();remotePlayback.volume=0;remotePlayback.srcObject=new MediaStream([e.track]);remotePlayback.play().catch(e=>errors.push(e.message));};
   remote.addTrack(input.stream.getAudioTracks()[0],input.stream);voice=new Voice(e=>{if(e.type==='error')errors.push(String(e.message));});await voice.configure({...defaultAudioSettings,noise:'off',volume:0,ducking:0,receiveAutoGain:false});await voice.connect({identity:'synthetic-only'},'',false);
   deliver({type:'state',own:1,members:[{id:1,channel:1}],channels:[{id:1}]});const offer=await remote.createOffer();await remote.setLocalDescription(offer);deliver({type:'offer',description:offer});
   const end=Date.now()+8000;while(Date.now()<end&&(voice.peer.connectionState!=='connected'||!voice.receivers.size||!remoteMeter||rms(remoteMeter)<.001))await pause(50);
   if(signalingError)throw Error(signalingError);if(!voice.receivers.size||!remoteMeter)throw Error('loopback did not receive audio');
   const receiver=[...voice.receivers.values()][0],player=[...voice.elements.values()][0],meter=voice.context.createAnalyser();voice.context.createMediaStreamSource(receiver.stream).connect(meter);
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
   voice.setMute(false,true);const muted=await read();voice.setMute(false,false);for(const audio of voice.elements.values())audio.muted=true;const restored=await read();
   const helperRms=rms(helperMeter);helperSource.disconnect();helperMeter.disconnect();zero.disconnect();
   const state=voice.peer.connectionState,count=voice.elements.size,helperTrack=helper.srcObject.getAudioTracks()[0],snapshot=voice.diagnostics();voice.close();const closedSnapshot=voice.diagnostics();voice=undefined;
   return {state,snapshot,closedSnapshot,repeatedEvents,reusedPlayer,players:count,heard,sent,muted,restored,helperRms,helperReleased:helperTrack.readyState==='ended',errors};
  }finally{voice?.close();remotePlayback?.pause();if(remotePlayback)remotePlayback.srcObject=null;remote.close();source.stop();input.stream.getTracks().forEach(t=>t.stop());await context.close();window.fetch=original.fetch;window.WebSocket=original.WebSocket;window.Audio=original.Audio;navigator.mediaDevices.getUserMedia=original.getUserMedia;}
 });
 expect(result.snapshot).toEqual({context:'running',processing:'off',receivers:1,players:1,pausedPlayers:0});expect(result.closedSnapshot).toEqual({context:'inactive',processing:'inactive',receivers:0,players:0,pausedPlayers:0});expect(result.errors).toEqual([]);expect(result.state).toBe('connected');expect(result.repeatedEvents).toBe(1);expect(result.reusedPlayer).toBe(true);expect(result.players).toBe(1);expect(result.heard).toBeGreaterThan(.001);expect(result.sent).toBeGreaterThan(.001);expect(result.muted).toBeLessThan(.00001);expect(result.restored).toBeGreaterThan(.001);expect(result.helperRms).toBe(0);expect(result.helperReleased).toBe(true);
});

test('real audio graph retains output attenuation and mute when element volume is ignored',async({page})=>{
 await page.goto('/tests/browser/private-messaging.html');await page.waitForFunction(()=>!!window.audioFixture);
 test.skip(!await page.evaluate(()=>typeof AudioContext==='function'),'This local browser build does not provide AudioContext; physical mobile validation remains required.');
 await page.locator('body').click();
 const result=await page.evaluate(async()=>{
  const {createReceiver}=window.audioFixture,OriginalAudio=window.Audio,audios=[];
  // Reproduce only the ignored element-volume behavior; the graph below is real Web Audio.
  window.Audio=class{constructor(){audios.push(this);}get volume(){return 1;}set volume(_){}get muted(){return false;}set muted(_){}play(){this.plays=(this.plays??0)+1;return Promise.resolve();}pause(){}};
  const context=new AudioContext(),source=context.createOscillator(),input=context.createMediaStreamDestination(),silent=context.createGain();silent.gain.value=0;
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
