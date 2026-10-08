import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/notification-sounds.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {NotificationSounds,readSoundSettings}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('sound preferences tolerate malformed storage, clamp volume and preserve explicit disable',()=>{
  globalThis.localStorage={getItem:()=>'{'};assert.deepEqual(readSoundSettings(),{enabled:true,volume:.15});
  localStorage.getItem=()=>'{"enabled":false,"volume":99}';assert.deepEqual(readSoundSettings(),{enabled:false,volume:1});
});
test('tones need a user-armed context, obey mute/volume and coalesce repeated events',()=>{
  const contexts=[],tones=[];
  globalThis.AudioContext=class {
    state='suspended';currentTime=1;
    constructor(){contexts.push(this);}resume(){this.state='running';return Promise.resolve();}close(){this.closed=true;return Promise.resolve();}
    createOscillator(){const osc={frequency:{value:0},connect(){},disconnect(){this.released=true;},start(t){this.startTime=t;},stop(t){this.stopTime=t;}};tones.push(osc);return osc;}
    createGain(){return {gain:{setValueAtTime(){},linearRampToValueAtTime(v){assert.ok(v>0&&v<=.15);},exponentialRampToValueAtTime(){}},connect(){},disconnect(){}};}
  };
  const sound=new NotificationSounds({enabled:true,volume:.15});sound.play('poke');assert.equal(contexts.length,0);sound.arm();sound.play('poke');assert.equal(tones.length,3);sound.play('poke');assert.equal(tones.length,3);
  for(const tone of tones){assert.ok(tone.stopTime-tone.startTime<.1);tone.onended();assert.ok(tone.released);}
  sound.configure({enabled:false,volume:1});sound.play('connect');assert.equal(tones.length,3);sound.configure({enabled:true,volume:0});sound.play('leave');assert.equal(tones.length,3);sound.close();assert.ok(contexts[0].closed);
});
