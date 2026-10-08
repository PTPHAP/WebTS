import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/voice.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, {compilerOptions:{module:ts.ModuleKind.ESNext, target:ts.ScriptTarget.ES2022}}).outputText.replace("import('./noise')",'globalThis.loadFixtureNoise()');
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
  globalThis.window = {setInterval:callback=>{timers.push(callback);return timers.length;}};
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
  globalThis.loadFixtureNoise=async()=>({createNoise:async(_,keyboard)=>{const node={input:{},output:{connect(){}},mode:keyboard?'keyboard':'rnnoise',destroy(){this.destroyed=true;}};processors.push(node);return node;}});
  globalThis.WebSocket = class {
    static OPEN=1;
    readyState=1;
    constructor(){sockets.push(this);queueMicrotask(()=>this.onopen?.());}
    send(message){sent.push(JSON.parse(message));}
    close(){this.readyState=3;}
  };
  return {sent,tracks,captured,constraints,contexts,sockets,processors,connections,tick:()=>timers.forEach(callback=>callback()),focus:value=>{focused=value;},requests:()=>requests};
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
  const env=browser(), voice=new Voice(()=>{});
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
test('default local keyboard chain feeds the send track; native NS stays off and failures do not gate speech',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));await voice.connect({identity:'fixture'},'',false);
  assert.equal(env.constraints[0].audio.noiseSuppression,false);assert.equal(env.constraints[0].audio.echoCancellation,true);
  assert.ok(env.connections.includes(env.processors[0].input));assert.equal(events.find(e=>e.type==='audio_processing').noise,'keyboard');
  const node=env.processors[0];node.mode='rnnoise';node.onchange('fixture keyboard failure');assert.equal(events.filter(e=>e.type==='audio_processing').at(-1).noise,'rnnoise');assert.equal(env.tracks[0].enabled,true);
  node.onerror();assert.equal(events.filter(e=>e.type==='audio_processing').at(-1).noise,'off');assert.ok(node.destroyed);env.tick();assert.equal(env.tracks[0].enabled,true);
  voice.close();assert.ok(env.captured.every(t=>t.stopped));
});
test('local initialization failure is visible and microphone remains open without silently enabling native NS',async()=>{
  const env=browser(),events=[],voice=new Voice(e=>events.push(e));globalThis.loadFixtureNoise=async()=>({createNoise:async()=>{throw Error('model unavailable');}});
  await voice.connect({identity:'fixture'},'',false);assert.equal(events.find(e=>e.type==='audio_processing').noise,'off');assert.ok(events.some(e=>e.type==='notice'&&e.message.includes('关闭降噪')));assert.equal(env.constraints[0].audio.noiseSuppression,false);assert.equal(env.tracks[0].enabled,true);voice.close();
});
