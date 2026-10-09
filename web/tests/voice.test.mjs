import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/voice.ts', import.meta.url), 'utf8');
const qualityCode=ts.transpileModule(await readFile(new URL('../src/connection-quality.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const qualityURL='data:text/javascript;base64,'+Buffer.from(qualityCode).toString('base64');
const settingsCode=ts.transpileModule(await readFile(new URL('../src/audio-settings.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const settingsURL='data:text/javascript;base64,'+Buffer.from(settingsCode).toString('base64');
const receiveCode=ts.transpileModule(await readFile(new URL('../src/receive-audio.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const receiveURL='data:text/javascript;base64,'+Buffer.from(receiveCode).toString('base64');
const compiled = ts.transpileModule(source, {compilerOptions:{module:ts.ModuleKind.ESNext, target:ts.ScriptTarget.ES2022}}).outputText.replace('./audio-settings',settingsURL).replace('./receive-audio',receiveURL).replace('./connection-quality',qualityURL).replace("import('./noise')",'globalThis.loadFixtureNoise()');
const {Voice} = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

function browser(microphoneError) {
  const sent = [], tracks = [], timers = [], captured=[],constraints=[],contexts=[],sockets=[],processors=[],connections=[];
  let requests = 0, focused = true, sampleFailure=false;
  const track = () => ({readyState:'live',enabled:true, stopped:false, stop(){this.stopped=true;}, clone:track,getSettings:()=>({noiseSuppression:true}),applyConstraints:async()=>{}});
  globalThis.fetch = async () => ({ok:true, json:async()=>({})});
  Object.defineProperty(globalThis, 'navigator', {configurable:true, value:{mediaDevices:{getUserMedia:async options=>{
    constraints.push(options);requests++;if(microphoneError)throw microphoneError;const input=track();captured.push(input);return new MediaStream([input]);
  }}}});
  globalThis.location = {protocol:'https:', host:'fixture.example'};
  globalThis.document = {hasFocus:()=>focused};
  const delays=new Map();let clock=0;globalThis.window = {setInterval:callback=>{timers.push(callback);return timers.length;},setTimeout:(callback,ms)=>{const id=++clock;delays.set(id,{callback,ms});return id;},clearTimeout:id=>delays.delete(id)};
  globalThis.clearInterval = ()=>{};
  globalThis.MediaStream = class {constructor(tracks){this.tracks=tracks;}getAudioTracks(){return this.tracks;}getTracks(){return this.tracks;}};
  globalThis.RTCPeerConnection = class {addTrack(track){tracks.push(track);return {track,replaceTrack:async function(next){this.track=next;tracks.push(next);}};}close(){}};
  globalThis.AudioContext = class {
    constructor(options){this.options=options;this.state='suspended';this.resumes=0;contexts.push(this);}
    createMediaStreamSource(stream){return {connect(target){target.input=stream;connections.push(target);},disconnect(){}};}
    createGain(){this.gain={gain:{value:1},connect(){}};return this.gain;}
    createMediaStreamDestination(){const output=track();captured.push(output);return {stream:new MediaStream([output])};}
    createAnalyser(){return {fftSize:512,getFloatTimeDomainData(data){if(sampleFailure)throw Error('sample unavailable');data.fill(this.input?.getAudioTracks()[0]?.sample??0);}};}
    close(){this.closed=true;this.state='closed';} resume(){this.resumes++;this.state='running';return Promise.resolve();}
  };
  globalThis.loadFixtureNoise=async()=>({createNoise:async(_,keyboard,voiceOnly)=>{const node={input:{},output:{connect(){}},mode:keyboard?'keyboard':'rnnoise',voiceOnly,destroy(){this.destroyed=true;}};processors.push(node);return node;}});
  globalThis.WebSocket = class {
    static OPEN=1;
    readyState=1;
    constructor(){sockets.push(this);queueMicrotask(()=>this.onopen?.());}
    send(message){sent.push(JSON.parse(message));}
    close(){this.readyState=3;}
  };
  return {sent,tracks,captured,constraints,contexts,sockets,processors,connections,delays,wait:async()=>{const [id,task]=delays.entries().next().value??[];if(task){delays.delete(id);await task.callback();}},message:async value=>{sockets.at(-1).onmessage?.({data:JSON.stringify(value)});for(let i=0;i<15;i++)await Promise.resolve();},tick:()=>timers.forEach(callback=>callback()),focus:value=>{focused=value;},requests:()=>requests,sampleFailure:value=>sampleFailure=value};
}
test('AFK pauses both PTT and free speech, preserves mute and restores state after reconnect',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));
  await voice.connect({identity:'fixture'},'',false);
  await env.message({type:'state',own:1,members:[{id:1,channel:1,away:false}],channels:[{id:1}]});
  voice.setAway(true,'吃饭中');assert.equal(env.tracks.at(-1).enabled,false);
  let request=env.sent.findLast(e=>e.action==='away');assert.equal(request.text,'吃饭中');
  assert.equal(env.sent.findLast(e=>e.type==='transmit').pre_roll,false);
  voice.setMode('ptt');voice.press(true);assert.equal(env.tracks.at(-1).enabled,false);
  await env.message({type:'result',id:request.id,ok:true});
  await env.message({type:'state',own:1,members:[{id:1,channel:1,away:true,awayMessage:'吃饭中'}],channels:[{id:1}]});
  await env.message({type:'disconnected',retryable:true});await env.wait();
  await env.message({type:'state',own:2,members:[{id:2,channel:1,away:false}],channels:[{id:1}]});
  request=env.sent.findLast(e=>e.action==='away');assert.equal(request.enabled,true);assert.equal(request.text,'吃饭中');assert.equal(env.tracks.at(-1).enabled,false);
  await env.message({type:'result',id:request.id,ok:true});
  voice.setMute(true,false);voice.setAway(false);voice.press(true);assert.equal(env.tracks.at(-1).enabled,false);
  request=env.sent.findLast(e=>e.action==='away');await env.message({type:'result',id:request.id,ok:true});
  voice.setMute(false,false);voice.press(true);assert.equal(env.tracks.at(-1).enabled,true);voice.close();
});
test('server rejection of AFK rolls back local capture state and manual disconnect clears AFK',async()=>{
  const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);voice.configure({noise:'off'});
  await env.message({type:'state',own:1,members:[{id:1,channel:1,away:false}],channels:[{id:1}]});
  voice.setAway(true,'暂时离开');const request=env.sent.findLast(e=>e.action==='away');
  await env.message({type:'result',id:request.id,ok:false});assert.equal(env.tracks.at(-1).enabled,true);
  voice.setAway(true);voice.close();await voice.connect({identity:'fixture'},'',false);
  await env.message({type:'state',own:1,members:[{id:1,channel:1,away:false}],channels:[{id:1}]});
  assert.equal(env.tracks.at(-1).enabled,true);voice.close();
});

test('microphone denial explains browser permission and listen-only recovery', async()=>{
  browser(new DOMException('Permission denied', 'NotAllowedError'));
  const voice=new Voice(()=>{});
  await assert.rejects(voice.connect({identity:'fixture'},'',false), /麦克风权限.*仅收听/);
  voice.close();
});

test('unprocessed microphone meter resumes its context instead of remaining silent',async()=>{
  const env=browser(),voice=new Voice(()=>{});voice.configure({noise:'off',keyboard:false,voiceOnly:false,echo:true,autoGain:true,gain:1,volume:1});
  await voice.connect({identity:'fixture'},'',false);
  assert.equal(env.contexts[0].state,'running');assert.ok(env.contexts[0].resumes>0);voice.close();
});

test('a paused audio graph disables sending and resumes when the browser interrupts capture',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',false);
  env.contexts[0].state='suspended';env.contexts[0].resume=()=>Promise.resolve();env.contexts[0].onstatechange?.();
  assert.equal(env.tracks[0].enabled,false);assert.ok(events.some(e=>e.type==='audio_capture'&&e.status==='paused'));
  env.contexts[0].state='running';env.contexts[0].onstatechange?.();assert.equal(env.tracks[0].enabled,true);voice.close();
});

test('listen-only audio also recovers a suspended context and removes wake listeners on close',async()=>{
 const env=browser(),events=[],handlers=new Set();window.addEventListener=(_,f)=>handlers.add(f);window.removeEventListener=(_,f)=>handlers.delete(f);
 const voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',true);const context=env.contexts[0];context.state='suspended';context.onstatechange();await Promise.resolve();assert.equal(context.state,'running');assert.ok(events.some(e=>e.type==='audio_capture'&&e.status==='paused'));assert.ok(handlers.size>0);voice.close();assert.equal(handlers.size,0);assert.equal(context.onstatechange,null);
});

test('microphone mute pauses sending and ended device restores capture without reconnecting',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',false);
  const input=env.captured[0];input.muted=true;input.onmute?.();assert.ok(events.some(e=>e.type==='audio_capture'&&e.status==='muted'));assert.equal(env.tracks[0].enabled,false);
  input.muted=false;input.onunmute?.();assert.equal(env.tracks[0].enabled,true);
  input.readyState='ended';input.onended?.();await voice.microphoneTask;assert.ok(!events.some(e=>e.type==='disconnected'));assert.equal(env.requests(),2);assert.equal(env.sockets[0].readyState,1);assert.equal(env.delays.size,0);voice.close();
});

test('input meter still moves when voice recognition intentionally produces silence',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',false);
  env.captured[0].sample=.05;env.tick();const level=events.filter(e=>e.type==='level').at(-1);
  assert.ok(level.input_level>.1);assert.equal(level.level,0);assert.equal(env.tracks[0].enabled,true);voice.close();
});

test('autoplay resume waiting for a gesture cannot stall TS connect; stale device callbacks cannot reconnect',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));AudioContext.prototype.resume=()=>new Promise(()=>{});
  await voice.connect({identity:'fixture'},'',false);assert.equal(env.sockets.length,1);assert.equal(env.tracks[0].enabled,false);
  assert.ok(events.some(e=>e.type==='audio_capture'&&e.status==='paused'));
  const old=env.captured[0],ended=old.onended;await voice.connect({identity:'replacement'},'',true);old.readyState='ended';ended();assert.equal(env.delays.size,0);assert.equal(old.onended,null);voice.close();
});

test('speaking ducks remote audio without undoing per-member volume, mute or live settings',async()=>{
 const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);
 const audio={volume:0,muted:false,pause(){}};voice.elements.set('remote',audio);voice.tracks.set('remote',2);voice.volume(2,.6);voice.outputVolume(.8);
 assert.equal(audio.volume,.48);env.processors[0].onspeech(true);assert.ok(Math.abs(audio.volume-.48*.65)<1e-9);
 voice.setMute(true,false);assert.equal(audio.volume,.48);voice.setMute(false,true);assert.equal(audio.muted,true);
 voice.configure({...voice.settings,ducking:0});assert.equal(audio.volume,.48);voice.close();
});

test('typing suppression sends only a local timing marker and stops after disconnect',async()=>{
 const env=browser(),handlers=new Set();window.addEventListener=(type,handler)=>{if(type==='keydown')handlers.add(handler);};window.removeEventListener=(type,handler)=>handlers.delete(handler);
 const voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);let marks=0;env.processors[0].typing=()=>marks++;
 for(const handler of handlers)handler({code:'KeyA'});assert.equal(marks,1);assert.ok(!env.sent.some(e=>e.type==='typing'||e.code));
 voice.setMode('ptt');voice.press(true);for(const handler of handlers)handler({code:'KeyV'});assert.equal(marks,1,'PTT activation must never suppress the first syllable as typing');voice.setMode('open');
 voice.configure({...voice.settings,typing:false});for(const handler of handlers)handler({code:'KeyB'});assert.equal(marks,1);voice.close();assert.equal(handlers.size,0);
});

test('mouse clicks use a local suppression timing hint, preserve PTT and detach on close',async()=>{
 const env=browser(),handlers=new Set();window.addEventListener=(type,handler)=>{if(type==='pointerdown')handlers.add(handler);};window.removeEventListener=(_,handler)=>handlers.delete(handler);
 const voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);let marks=0;env.processors[0].typing=()=>marks++;
 for(const handler of handlers)handler();assert.equal(marks,1);assert.ok(!env.sent.some(e=>e.type==='typing'||e.type==='pointerdown'));
 voice.setMode('ptt');voice.press(true);for(const handler of handlers)handler();assert.equal(marks,1);voice.close();assert.equal(handlers.size,0);
});

test('gain processing preserves continuous mic and releases raw and processed tracks',async()=>{
  const env=browser(),voice=new Voice(()=>{});voice.configure({noise:'off',echo:false,autoGain:false,gain:1.4,volume:0.6});
  await voice.connect({identity:'fixture'},'test-device',false);assert.equal(env.constraints[0].audio.echoCancellation,false);assert.equal(env.constraints[0].audio.noiseSuppression,false);assert.equal(env.constraints[0].audio.autoGainControl,false);assert.equal(env.contexts[0].options.sampleRate,48000);assert.equal(env.contexts[0].gain.gain.value,1.4);assert.equal(env.tracks[0].enabled,true);
  voice.close();assert.ok(env.captured.every(t=>t.stopped));assert.ok(env.contexts.every(c=>c.closed));
});
test('cancelled config fetch never opens microphone or creates another socket',async()=>{
  const env=browser(),voice=new Voice(()=>{});let resolve;globalThis.fetch=()=>new Promise(r=>resolve=r);const old=voice.connect({identity:'old'},'',false);voice.close();globalThis.fetch=async()=>({ok:true,json:async()=>({})});await voice.connect({identity:'new'},'',true);resolve({ok:true,json:async()=>({})});await old;assert.equal(env.requests(),0);assert.equal(env.sockets.length,1);assert.equal(env.sockets[0].readyState,1);voice.close();
});
test('cancelled microphone failure cannot close replacement connection',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));let reject;let started;const captureStarted=new Promise(r=>started=r);navigator.mediaDevices.getUserMedia=()=>{started();return new Promise((_,r)=>reject=r);};const old=voice.connect({identity:'old'},'',false);await captureStarted;await voice.connect({identity:'new'},'',true);reject(new DOMException('Permission denied','NotAllowedError'));await old;assert.equal(env.sockets.at(-1).readyState,1);assert.ok(!events.some(e=>e.type==='error'));voice.close();
});

test('missing or busy microphone offers device selection or listen-only', async()=>{
  for (const [name, message] of [['NotFoundError',/未找到.*仅收听/],['NotReadableError',/无法读取.*仅收听/]]) {
    browser(new DOMException('browser detail',name));
    const voice=new Voice(()=>{});
    await assert.rejects(voice.connect({identity:'fixture'},'',false),message);
    voice.close();
  }
});

test('turning off strict voice-only processing still ends TS speech in pauses; focused PTT bypasses detection', async()=>{
  const env=browser(), voice=new Voice(()=>{});voice.configure({noise:'rnnoise',keyboard:true,voiceOnly:false,echo:true,autoGain:true,gain:1,volume:1});
  await voice.connect({identity:'fixture'},'',false);
  assert.equal(env.tracks[0].enabled,true,'default microphone must be open');
  assert.equal(env.sent.filter(event=>event.type==='transmit').at(-1)?.enabled,false,'continuous capture must not mean continuous TS speech');
  assert.equal(env.sent.filter(event=>event.type==='transmit').at(-1)?.pre_roll,true,'automatic speech needs a bounded leading-audio buffer before recognition arrives');
  await env.message({type:'state',own:1,members:[{id:1,channel:1}],channels:[{id:1,name:'default'}]});
  env.processors[0].onspeech(true);assert.equal(env.sent.filter(event=>event.type==='transmit').at(-1)?.enabled,true);
  env.processors[0].onspeech(false);await env.wait();assert.equal(env.sent.filter(event=>event.type==='transmit').at(-1)?.enabled,false,'native TS must receive an end marker during a pause');
  env.tick();
  assert.equal(env.tracks[0].enabled,true,'zero input level must not gate speech');
  voice.setMute(true,false);assert.equal(env.tracks[0].enabled,false);
  voice.setMute(false,false);assert.equal(env.tracks[0].enabled,true);
  voice.setMode('ptt');assert.equal(env.tracks[0].enabled,false);
  assert.equal(env.sent.filter(event=>event.type==='transmit').at(-1)?.pre_roll,false,'PTT must clear automatic lookback even if it was already silent');
  voice.press(true);assert.equal(env.tracks[0].enabled,true);
  env.focus(false);voice.press(true);assert.equal(env.tracks[0].enabled,false);
  voice.setMode('open');assert.equal(env.tracks[0].enabled,true);
  voice.close();assert.equal(env.tracks[0].stopped,true);
});

test('listen-only never requests microphone or announces transmission', async()=>{
  const env=browser(new DOMException('Permission denied','NotAllowedError')), voice=new Voice(()=>{});
  await voice.connect({identity:'fixture'},'',true);
  voice.press(true);env.tick();
  assert.equal(env.requests(),0);assert.equal(env.tracks.length,0);
  assert.ok(env.sent.some(event=>event.identity==='fixture'));
  assert.ok(!env.sent.some(event=>event.type==='transmit'&&event.enabled));
  voice.close();
});
test('quality polling does not overlap or report stale results after disconnect',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));let resolve,reads=0;
  RTCPeerConnection.prototype.getStats=()=>{reads++;return new Promise(r=>resolve=r);};
  await voice.connect({identity:'fixture'},'',true);env.tick();env.tick();assert.equal(reads,1);
  voice.close();resolve(new Map([['pair',{type:'candidate-pair',state:'succeeded',nominated:true,currentRoundTripTime:.02}]]));await Promise.resolve();await Promise.resolve();
  assert.ok(!events.some(e=>e.type==='quality'),'late stats cannot populate a closed or replacement connection');
});
test('explicit strict human voice chain feeds the send track; recognition failure safely blocks sending',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));voice.configure({...voice.settings,voiceOnly:true});await voice.connect({identity:'fixture'},'',false);
  assert.equal(env.constraints[0].audio.noiseSuppression,false);assert.equal(env.constraints[0].audio.echoCancellation,true);
  assert.ok(env.connections.includes(env.processors[0].input));assert.equal(events.find(e=>e.type==='audio_processing').noise,'keyboard');
  assert.equal(env.processors[0].voiceOnly,true);assert.equal(events.find(e=>e.type==='audio_processing').voice_only,true);
  const node=env.processors[0];node.mode='rnnoise';node.onchange('fixture keyboard failure');assert.equal(events.filter(e=>e.type==='audio_processing').at(-1).noise,'rnnoise');assert.equal(env.tracks[0].enabled,true);
  const oldError=node.onerror;node.onerror();assert.equal(events.filter(e=>e.type==='audio_processing').at(-1).noise,'blocked');assert.ok(node.destroyed);env.tick();assert.equal(env.tracks[0].enabled,false);assert.equal(env.contexts[0].gain.gain.value,0);assert.equal(env.sent.filter(e=>e.type==='transmit').at(-1).enabled,false);
  voice.setMute(true,false);voice.setMute(false,false);voice.setMode('ptt');voice.press(true);assert.equal(env.tracks[0].enabled,false,'mute/PTT cannot bypass a failed recognizer');
  await voice.connect({identity:'fixture'},'',false);voice.setMode('open');assert.equal(env.tracks.at(-1).enabled,true);oldError();assert.equal(env.tracks.at(-1).enabled,true,'stale processor failure cannot block a replacement connection');
  voice.close();assert.ok(env.captured.every(t=>t.stopped));
});
test('human voice initialization failure permits receiving but never falls back to raw microphone',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));globalThis.loadFixtureNoise=async()=>({createNoise:async()=>{throw Error('model unavailable');}});
  await voice.connect({identity:'fixture'},'',false);assert.equal(events.find(e=>e.type==='audio_processing').noise,'blocked');assert.ok(events.some(e=>e.type==='notice'&&e.message.includes('暂停麦克风')));assert.equal(env.constraints[0].audio.noiseSuppression,false);assert.equal(env.tracks[0].enabled,false);assert.equal(env.contexts[0].gain.gain.value,0);assert.ok(!env.sent.some(e=>e.type==='transmit'&&e.enabled));voice.close();
});

test('disabled strict filtering still fails safely rather than transmitting raw mic after a model failure',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));voice.configure({noise:'rnnoise',keyboard:true,voiceOnly:false,echo:true,autoGain:true,gain:1,volume:1});
  await voice.connect({identity:'fixture'},'',false);assert.equal(env.processors[0].voiceOnly,false);env.processors[0].onerror();assert.equal(events.filter(e=>e.type==='audio_processing').at(-1).noise,'blocked');assert.equal(env.tracks[0].enabled,false);assert.equal(env.contexts[0].gain.gain.value,0);voice.close();
});


test('voice-only model activity controls TS transmit while the capture track stays alive',async()=>{
  const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);
  assert.equal(env.sent.filter(e=>e.type==='transmit').at(-1).enabled,false,'silence must not open TS voice');
  assert.equal(env.tracks[0].enabled,true,'keep processing so speech attack is not clipped');
  await env.message({type:'state',own:1,members:[{id:1,channel:1}],channels:[{id:1,name:'default'}]});
  const old=env.processors[0];old.onspeech(true);assert.equal(env.sent.at(-1).enabled,true);
  old.onspeech(false);await env.wait();assert.equal(env.sent.at(-1).enabled,false);
  voice.setMute(true,false);old.onspeech(true);assert.equal(env.tracks[0].enabled,false);voice.setMute(false,false);assert.equal(env.sent.filter(e=>e.type==='transmit').at(-1).enabled,true);
  voice.close();await voice.connect({identity:'other'},'',false);old.onspeech(true);assert.equal(env.sent.filter(e=>e.type==='transmit').at(-1).enabled,false);voice.close();
});

test('unexpected disconnect retries indefinitely with capped backoff and restores actual last channel',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture',password:'memory-only'},'',true);
  await env.message({type:'state',own:1,members:[{id:1,channel:7}],channels:[{id:7,name:'last'}]});
  env.sockets.at(-1).onclose();assert.equal(env.delays.size,1);
  for(let n=0;n<9;n++){assert.ok([...env.delays.values()][0].ms<=30000);await env.wait();env.sockets.at(-1).onclose();}
  await env.wait();await env.message({type:'state',own:2,members:[{id:2,channel:1}],channels:[{id:1,name:'default'},{id:7,name:'last'}]});
  assert.equal(events.find(e=>e.type==='restore_channel').channel,7);
  assert.ok(env.sent.some(e=>e.identity==='fixture'&&e.password==='memory-only'));
  voice.close();assert.equal(env.delays.size,0);assert.ok(env.captured.every(t=>t.stopped));
});

test('explicit nonretryable server closure and deliberate disconnect cancel reconnect',async()=>{
  const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',true);
  await env.message({type:'disconnected',retryable:false});assert.equal(env.delays.size,0);
  await voice.connect({identity:'fixture'},'',true);env.sockets.at(-1).onclose();assert.equal(env.delays.size,1);voice.close();assert.equal(env.delays.size,0);
});

test('unexpected closures always explain why and retain the retry verdict',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',true);
  await env.message({type:'disconnected',retryable:false});
  assert.match(events.find(e=>e.type==='disconnected').message,/连接.*停止/);assert.equal(env.delays.size,0);
  events.length=0;await voice.connect({identity:'fixture'},'',true);env.sockets.at(-1).onclose();
  assert.match(events.find(e=>e.type==='disconnected').message,/网页.*连接.*中断/);assert.equal(env.delays.size,1);voice.close();
});

test('gateway terminal reason and preceding error are shown without reconnecting revoked connections',async()=>{
  for(const direct of [true,false]){
    const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',true);
    if(!direct)await env.message({type:'error',message:'连接已撤销，请检查账号或管理员配置'});
    await env.message({type:'disconnected',retryable:false,...(direct?{message:'连接已撤销，请检查账号或管理员配置'}:{})});
    assert.match(events.find(e=>e.type==='disconnected').message,/连接已撤销/);assert.equal(env.delays.size,0);voice.close();
  }
});

test('failed media transport reports a reason and reconnects',async()=>{
  browser();const events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',true);
  voice.peer.connectionState='failed';voice.peer.onconnectionstatechange();
  assert.match(events.find(e=>e.type==='disconnected').message,/语音.*连接.*中断/);assert.ok(events.some(e=>e.type==='reconnecting'&&e.active));voice.close();
});


test('HTTP auth expiry stops retries; an offline gateway stays retryable; no stale close survives replacement',async()=>{
  const env=browser(),voice=new Voice(()=>{});globalThis.fetch=async()=>{throw Error('offline');};await voice.connect({identity:'fixture'},'',true);assert.equal(env.delays.size,1);
  globalThis.fetch=async()=>({ok:false,status:401});await env.wait();assert.equal(env.delays.size,0);
  globalThis.fetch=async()=>({ok:true,json:async()=>({})});await voice.connect({identity:'new'},'',true);const old=env.sockets.at(-1).onclose;await voice.connect({identity:'newer'},'',true);old();assert.equal(env.delays.size,0);voice.close();
});

test('channel restore survives another drop and deleted channels fall back with notice',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',true);
  await env.message({type:'state',own:1,members:[{id:1,channel:7}],channels:[{id:7,name:'last'}]});
  for(let i=0;i<2;i++){env.sockets.at(-1).onclose();await env.wait();await env.message({type:'state',own:2,members:[{id:2,channel:1}],channels:[{id:1,name:'default'},{id:7,name:'last'}]});}
  assert.equal(events.filter(e=>e.type==='restore_channel').length,2);
  env.sockets.at(-1).onclose();await env.wait();await env.message({type:'state',own:3,members:[{id:3,channel:1}],channels:[{id:1,name:'default'}]});assert.ok(events.some(e=>e.type==='notice'&&e.message.includes('已不存在')));voice.close();
});


test('speech changes during a slow TS handshake never flood queued control requests',async()=>{
  const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);
  for(let i=0;i<60;i++){env.processors[0].onspeech(true);env.processors[0].onspeech(false);await env.wait();}
  assert.equal(env.sent.filter(e=>e.type==='transmit').length,1);
  await env.message({type:'state',own:1,members:[{id:1,channel:1}],channels:[{id:1,name:'default'}]});assert.equal(env.sent.filter(e=>e.type==='transmit').at(-1).enabled,false);voice.close();
});

test('AFK confirmation emits one sound per accepted transition, never for rejection or reconnect',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',false);
  const state={type:'state',own:1,members:[{id:1,channel:1,away:false}],channels:[{id:1}]};await env.message(state);
  voice.setAway(true,'休息');assert.equal(events.filter(e=>e.type==='afk_sound').length,0);
  let request=env.sent.findLast(e=>e.action==='away');await env.message({type:'result',id:request.id,ok:true});
  assert.deepEqual(events.filter(e=>e.type==='afk_sound').map(e=>e.enabled),[true]);
  await env.message({...state,members:[{id:1,channel:1,away:true}]});
  await env.message({type:'disconnected',retryable:true});await env.wait();await env.message(state);
  request=env.sent.findLast(e=>e.action==='away');await env.message({type:'result',id:request.id,ok:true});
  assert.equal(events.filter(e=>e.type==='afk_sound').length,1);
  voice.setAway(false);request=env.sent.findLast(e=>e.action==='away');await env.message({type:'result',id:request.id,ok:false});
  assert.equal(events.filter(e=>e.type==='afk_sound').length,1);
  voice.setAway(false);request=env.sent.findLast(e=>e.action==='away');await env.message({type:'result',id:request.id,ok:true});
  assert.deepEqual(events.filter(e=>e.type==='afk_sound').map(e=>e.enabled),[true,false]);voice.close();
});

test('ended microphone keeps TS socket, peer, receive audio and original channel alive',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',false);
  await env.message({type:'state',own:1,members:[{id:1,channel:7}],channels:[{id:7,name:'original'}]});
  const socket=env.sockets[0],peer=voice.peer,context=env.contexts[0];
  env.captured[0].readyState='ended';env.captured[0].onended();
  assert.equal(socket.readyState,1,'microphone failure must not disconnect TeamSpeak');
  assert.equal(voice.peer,peer);assert.equal(context.closed,undefined);assert.equal(voice.currentChannel,7);
  assert.ok(!events.some(e=>e.type==='disconnected'));
  assert.ok(!env.sent.some(e=>e.type==='disconnect'&&socket.readyState===3));voice.close();
});

test('sampling failure retries only the microphone with bounded backoff and keeps AI protection',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'selected-device',false);
  await env.message({type:'state',own:1,members:[{id:1,channel:7}],channels:[{id:7}]});
  const socket=voice.socket,node=voice.noiseNode;
  navigator.mediaDevices.getUserMedia=async()=>{throw new DOMException('device busy','NotReadableError');};
  env.sampleFailure(true);env.tick();await voice.microphoneTask;
  assert.equal(voice.socket,socket);assert.equal(socket.readyState,1);assert.equal(voice.noiseNode,node);assert.equal(voice.currentChannel,7);
  assert.equal(env.tracks[0].enabled,false);assert.equal(env.sent.findLast(e=>e.type==='transmit').pre_roll,false);
  assert.equal([...env.delays.values()][0].ms,2000);
  await env.wait();await voice.microphoneTask;assert.equal([...env.delays.values()][0].ms,4000);
  assert.ok(!events.some(e=>e.type==='disconnected'));voice.close();assert.equal(env.delays.size,0);
});
test('recovered microphone uses the same processed send track and recognizer, without bypassing mute or AFK',async()=>{
  const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);
  const socket=voice.socket,peer=voice.peer,node=voice.noiseNode,send=voice.sendTrack;
  voice.setMute(true,false);env.captured[0].readyState='ended';env.captured[0].onended();await voice.microphoneTask;
  assert.equal(voice.socket,socket);assert.equal(voice.peer,peer);assert.equal(voice.sendTrack,send);assert.equal(voice.noiseNode,node);
  assert.equal(send.enabled,false);assert.equal(env.constraints.at(-1).audio.noiseSuppression,false);
  voice.setMute(false,false);assert.equal(send.enabled,true);node.onspeech(true);
  voice.setAway(true);assert.equal(send.enabled,false);voice.close();
});
test('late microphone recovery after disconnect or device change releases the stale stream',async()=>{
  for(const changed of [false,true]){
    const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);
    const replacement=new MediaStream([{...env.captured[0],stopped:false,stop(){this.stopped=true;}}]);let resolve;
    navigator.mediaDevices.getUserMedia=()=>new Promise(r=>resolve=r);
    env.captured[0].readyState='ended';env.captured[0].onended();const task=voice.microphoneTask;
    if(changed)voice.inputDevice('new-device');else voice.close();
    resolve(replacement);await task;
    assert.equal(replacement.getTracks()[0].stopped,true);
    if(changed){assert.equal(voice.intent.device,'new-device');assert.equal(voice.socket.readyState,1);}
    voice.close();
  }
});
test('unprocessed microphone recovery replaces only the outgoing sender track',async()=>{
  const env=browser(),voice=new Voice(()=>{});voice.configure({...voice.settings,noise:'off',gain:1});await voice.connect({identity:'fixture'},'',false);
  const socket=voice.socket,old=voice.sendTrack;env.captured[0].readyState='ended';env.captured[0].onended();await voice.microphoneTask;
  assert.equal(voice.socket,socket);assert.notEqual(voice.sendTrack,old);assert.equal(old.stopped,true);assert.equal(voice.sendTrack.enabled,true);voice.close();
});

test('capture timeout releases late microphone and permission denial stays paused until explicit recovery',async()=>{
  const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);
  let resolve;const old=env.captured[0];navigator.mediaDevices.getUserMedia=()=>new Promise(r=>resolve=r);
  old.readyState='ended';old.onended();const task=voice.microphoneTask;
  await env.wait();await task;assert.equal(voice.socket.readyState,1);assert.equal([...env.delays.values()][0].ms,2000);
  const track={readyState:'live',stopped:false,stop(){this.stopped=true;}};resolve(new MediaStream([track]));await Promise.resolve();assert.equal(track.stopped,true);
  navigator.mediaDevices.getUserMedia=async()=>{throw new DOMException('denied','NotAllowedError');};
  await env.wait();await voice.microphoneTask;assert.equal(env.delays.size,0);assert.equal(voice.sendTrack.enabled,false);
  old.onunmute();voice.context.onstatechange();await Promise.resolve();assert.equal(voice.microphoneTask,undefined);
  assert.equal(voice.socket.readyState,1);voice.close();
});

test('recovering capture retains active receivers and processor failure cannot leak raw audio afterward',async()=>{
  const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);
  let destroyed=false;voice.receivers.set('remote',{destroy(){destroyed=true;},tick(){},setVolume(){}});
  voice.elements.set('remote',{pause(){},srcObject:{}});
  const old=env.captured[0],node=voice.noiseNode;
  old.readyState='ended';old.onended();await voice.microphoneTask;assert.equal(destroyed,false);assert.ok(voice.receivers.has('remote'));
  node.onerror();assert.equal(voice.sendTrack.enabled,false);assert.equal(voice.processingBlocked,true);
  voice.inputDevice('replacement');await voice.microphoneTask;assert.equal(voice.sendTrack.enabled,false);assert.equal(destroyed,false);voice.close();assert.equal(destroyed,true);
});

test('manual recovery reacquires a microphone that remains hardware-muted without disconnecting',async()=>{
  const env=browser(),voice=new Voice(()=>{});await voice.connect({identity:'fixture'},'',false);
  const socket=voice.socket;env.captured[0].muted=true;env.captured[0].onmute();assert.equal(voice.sendTrack.enabled,false);
  voice.resume();await voice.microphoneTask;
  assert.equal(env.requests(),2);assert.equal(voice.socket,socket);assert.equal(voice.sendTrack.enabled,true);voice.close();
});
