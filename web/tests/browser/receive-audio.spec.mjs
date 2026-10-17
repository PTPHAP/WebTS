import {test,expect} from '@playwright/test';

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
   navigator.mediaDevices.getUserMedia=async()=>input.stream.clone();window.fetch=(url,options)=>url==='/api/rtc'?Promise.resolve(new Response('{"iceServers":[]}')):original.fetch(url,options);
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
  const read=async()=>{await new Promise(r=>setTimeout(r,160));const data=new Float32Array(analyser.fftSize);analyser.getFloatTimeDomainData(data);return Math.sqrt(data.reduce((s,n)=>s+n*n,0)/data.length);};
  try{
   source.frequency.value=440;source.connect(input);source.start();await context.resume();receiver=createReceiver(context,input.stream.getAudioTracks()[0]);await receiver.play();
   meterSource=context.createMediaStreamSource(receiver.stream);meterSource.connect(analyser);analyser.connect(silent);silent.connect(context.destination);
   receiver.volume(.5);const half=await read();receiver.volume(.25);const quarter=await read();receiver.tick(true);const agc=await read();receiver.volume(0);const muted=await read();receiver.volume(.5);const restored=await read();
   return {half,quarter,agc,muted,restored,helperCopies:audios.filter(a=>a.srcObject&&a.plays&&a.volume!==0&&!a.muted&&a.srcObject.getAudioTracks()[0]?.enabled!==false).length};
  }finally{window.Audio=OriginalAudio;receiver?.destroy();meterSource?.disconnect();analyser.disconnect();silent.disconnect();source.stop();source.disconnect();input.stream.getTracks().forEach(t=>t.stop());input.disconnect();await context.close();}
 });
 expect(result.helperCopies).toBe(0);expect(result.half).toBeGreaterThan(.01);expect(result.quarter/result.half).toBeGreaterThan(.45);expect(result.quarter/result.half).toBeLessThan(.55);expect(result.agc).toBeGreaterThan(.01);expect(result.muted).toBeLessThan(.00001);expect(result.restored).toBeGreaterThan(.01);
});
