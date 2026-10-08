import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/voice.ts', import.meta.url), 'utf8');
const qualityCode=ts.transpileModule(await readFile(new URL('../src/connection-quality.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const qualityURL='data:text/javascript;base64,'+Buffer.from(qualityCode).toString('base64');
const compiled = ts.transpileModule(source, {compilerOptions:{module:ts.ModuleKind.ESNext, target:ts.ScriptTarget.ES2022}}).outputText.replace('./connection-quality',qualityURL).replace("import('./noise')",'globalThis.loadFixtureNoise()');
const {Voice} = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

function browser(microphoneError) {
  const sent = [], tracks = [], timers = [], captured=[],constraints=[],contexts=[],sockets=[],processors=[],connections=[];
  let requests = 0, focused = true;
  const track = () => ({enabled:true, stopped:false, stop(){this.stopped=true;}, clone:track,getSettings:()=>({noiseSuppression:true}),applyConstraints:async()=>{}});
  globalThis.fetch = async () => ({ok:true, json:async()=>({})});
  Object.defineProperty(globalThis, 'navigator', {configurable:true, value:{mediaDevices:{getUserMedia:async options=>{
    constraints.push(options);requests++;if(microphoneError)throw microphoneError;const input=track();captured.push(input);return new MediaStream([input]);
  }}}});
  globalThis.location = {protocol:'https:', host:'fixture.example'};
  globalThis.document = {hasFocus:()=>focused};
  const delays=new Map();let clock=0;globalThis.window = {setInterval:callback=>{timers.push(callback);return timers.length;},setTimeout:(callback,ms)=>{const id=++clock;delays.set(id,{callback,ms});return id;},clearTimeout:id=>delays.delete(id)};
  globalThis.clearInterval = ()=>{};
  globalThis.MediaStream = class {constructor(tracks){this.tracks=tracks;}getAudioTracks(){return this.tracks;}getTracks(){return this.tracks;}};
  globalThis.RTCPeerConnection = class {addTrack(track){tracks.push(track);}close(){}};
  globalThis.AudioContext = class {
    constructor(options){this.options=options;contexts.push(this);}
    createMediaStreamSource(){return {connect(target){connections.push(target);},disconnect(){}};}
    createGain(){this.gain={gain:{value:1},connect(){}};return this.gain;}
    createMediaStreamDestination(){const output=track();captured.push(output);return {stream:new MediaStream([output])};}
    createAnalyser(){return {fftSize:512,getFloatTimeDomainData:data=>data.fill(0)};}
    close(){this.closed=true;} resume(){}
  };
  globalThis.loadFixtureNoise=async()=>({createNoise:async(_,keyboard,voiceOnly)=>{const node={input:{},output:{connect(){}},mode:keyboard?'keyboard':'rnnoise',voiceOnly,destroy(){this.destroyed=true;}};processors.push(node);return node;}});
  globalThis.WebSocket = class {
    static OPEN=1;
    readyState=1;
    constructor(){sockets.push(this);queueMicrotask(()=>this.onopen?.());}
    send(message){sent.push(JSON.parse(message));}
    close(){this.readyState=3;}
  };
  return {sent,tracks,captured,constraints,contexts,sockets,processors,connections,delays,wait:async()=>{const [id,task]=delays.entries().next().value??[];if(task){delays.delete(id);await task.callback();}},message:async value=>{sockets.at(-1).onmessage?.({data:JSON.stringify(value)});for(let i=0;i<15;i++)await Promise.resolve();},tick:()=>timers.forEach(callback=>callback()),focus:value=>{focused=value;},requests:()=>requests};
}

test('microphone denial explains browser permission and listen-only recovery', async()=>{
  browser(new DOMException('Permission denied', 'NotAllowedError'));
  const voice=new Voice(()=>{});
  await assert.rejects(voice.connect({identity:'fixture'},'',false), /麦克风权限.*仅收听/);
  voice.close();
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

test('open microphone sends through silence; mute and focused PTT still control transmission', async()=>{
  const env=browser(), voice=new Voice(()=>{});voice.configure({noise:'rnnoise',keyboard:true,voiceOnly:false,echo:true,autoGain:true,gain:1,volume:1});
  await voice.connect({identity:'fixture'},'',false);
  assert.equal(env.tracks[0].enabled,true,'default microphone must be open');
  assert.equal(env.sent.filter(event=>event.type==='transmit').at(-1)?.enabled,true,'initial state reaches gateway');
  env.tick();
  assert.equal(env.tracks[0].enabled,true,'zero input level must not gate speech');
  voice.setMute(true,false);assert.equal(env.tracks[0].enabled,false);
  voice.setMute(false,false);assert.equal(env.tracks[0].enabled,true);
  voice.setMode('ptt');assert.equal(env.tracks[0].enabled,false);
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
test('default human voice chain feeds the send track; recognition failure safely blocks sending',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',false);
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

test('explicit continuous denoise keeps the prior raw fallback on model failure',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));voice.configure({noise:'rnnoise',keyboard:true,voiceOnly:false,echo:true,autoGain:true,gain:1,volume:1});
  await voice.connect({identity:'fixture'},'',false);assert.equal(env.processors[0].voiceOnly,false);env.processors[0].onerror();assert.equal(events.filter(e=>e.type==='audio_processing').at(-1).noise,'off');assert.equal(env.tracks[0].enabled,true);assert.equal(env.contexts[0].gain.gain.value,1);voice.close();
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
