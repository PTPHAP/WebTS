import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/voice.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, {compilerOptions:{module:ts.ModuleKind.ESNext, target:ts.ScriptTarget.ES2022}}).outputText;
const {Voice} = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

function browser(microphoneError) {
  const sent = [], tracks = [], timers = [];
  let requests = 0, focused = true;
  const track = () => ({enabled:true, stopped:false, stop(){this.stopped=true;}, clone:track});
  globalThis.fetch = async () => ({ok:true, json:async()=>({})});
  Object.defineProperty(globalThis, 'navigator', {configurable:true, value:{mediaDevices:{getUserMedia:async()=>{
    requests++; if(microphoneError)throw microphoneError;return new MediaStream([track()]);
  }}}});
  globalThis.location = {protocol:'https:', host:'fixture.example'};
  globalThis.document = {hasFocus:()=>focused};
  globalThis.window = {setInterval:callback=>{timers.push(callback);return timers.length;}};
  globalThis.clearInterval = ()=>{};
  globalThis.MediaStream = class {constructor(tracks){this.tracks=tracks;}getAudioTracks(){return this.tracks;}getTracks(){return this.tracks;}};
  globalThis.RTCPeerConnection = class {addTrack(track){tracks.push(track);}close(){}};
  globalThis.AudioContext = class {
    createMediaStreamSource(){return {connect(){}};}
    createAnalyser(){return {fftSize:512,getFloatTimeDomainData:data=>data.fill(0)};}
    close(){} resume(){}
  };
  globalThis.WebSocket = class {
    static OPEN=1;
    readyState=1;
    constructor(){queueMicrotask(()=>this.onopen?.());}
    send(message){sent.push(JSON.parse(message));}
    close(){this.readyState=3;}
  };
  return {sent, tracks, tick:()=>timers.forEach(callback=>callback()), focus:value=>{focused=value;}, requests:()=>requests};
}

test('microphone denial explains browser permission and listen-only recovery', async()=>{
  browser(new DOMException('Permission denied', 'NotAllowedError'));
  const voice=new Voice(()=>{});
  await assert.rejects(voice.connect({identity:'fixture'},'',false), /麦克风权限.*仅收听/);
  voice.close();
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
