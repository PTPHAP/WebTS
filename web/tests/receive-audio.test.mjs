import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/receive-audio.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {receiveGain,createReceiver}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));

test('decoder attachment contains only a disabled owned clone even when element volume reports success',async()=>{
 const audios=[];globalThis.Audio=class{constructor(){audios.push(this);}play(){return Promise.resolve();}pause(){}};
 globalThis.MediaStream=class{constructor(tracks){this.tracks=tracks;}getTracks(){return this.tracks;}};
 const silent={enabled:true,stop(){this.stopped=true;}},track={enabled:true,clone:()=>silent},node=()=>({connect(){},disconnect(){}}),context={createMediaStreamSource:node,createAnalyser:()=>({...node(),getFloatTimeDomainData:data=>data.fill(0)}),createGain:()=>({...node(),gain:{setTargetAtTime(){}}}),createDynamicsCompressor:()=>({...node(),threshold:{},knee:{},ratio:{},attack:{},release:{}}),createMediaStreamDestination:()=>({...node(),stream:new MediaStream([])})};
 const receiver=createReceiver(context,track);await receiver.play();const attached=audios[0].srcObject.getTracks()[0];
 assert.notEqual(attached,track,'no raw original track may reach the helper regardless of volume or mute implementation');assert.equal(attached.enabled,false);assert.equal(track.enabled,true);
 receiver.destroy();assert.equal(silent.stopped,true);assert.equal(track.stopped,undefined);
});

test('a browser that ignores element volume never plays a second unattenuated copy and output gain still controls it',async()=>{
 const audios=[],gains=[];globalThis.Audio=class{constructor(){audios.push(this);}set volume(_){}get volume(){return 1;}play(){this.plays=(this.plays??0)+1;return Promise.resolve();}pause(){}};
 globalThis.MediaStream=class{constructor(tracks){this.tracks=tracks;}getTracks(){return this.tracks;}};
 const track={id:'remote',clone:()=>({enabled:true,stop(){}})},output={stop(){}},node=()=>({connect(){},disconnect(){}}),context={currentTime:0,createMediaStreamSource:node,createAnalyser:()=>({...node(),getFloatTimeDomainData:data=>data.fill(.03)}),createGain:()=>{const gain={value:1,setTargetAtTime(value){this.value=value;}},n={...node(),gain};gains.push(n);return n;},createDynamicsCompressor:()=>({...node(),threshold:{},knee:{},ratio:{},attack:{},release:{}}),createMediaStreamDestination:()=>({...node(),stream:new MediaStream([output])})};
 const receiver=createReceiver(context,track);await receiver.play();
 assert.equal(audios.filter(a=>a.srcObject&&a.plays&&a.volume!==0&&!a.muted).length,0,'the decoder helper must not bypass the processed output on iOS-like volume behavior');
 receiver.volume(.25);assert.equal(gains.at(-1).gain.value,.25);receiver.tick(true);assert.equal(gains.at(-1).gain.value,.25,'receive AGC must not override manual output attenuation');
 receiver.volume(0);assert.equal(gains.at(-1).gain.value,0);receiver.volume(1);assert.equal(gains.at(-1).gain.value,1);
 receiver.destroy();assert.equal(track.stopped,undefined);
});

test('remote RTC decoding uses an isolated silent track and resumes without duplicate sound',async()=>{
 const audios=[];globalThis.Audio=class{constructor(){audios.push(this);}play(){this.plays=(this.plays??0)+1;return Promise.resolve();}pause(){this.paused=true;}};
 globalThis.MediaStream=class{constructor(tracks){this.tracks=tracks;}getTracks(){return this.tracks;}};
 const silent={enabled:true,stop(){this.stopped=true;}},track={id:'rtc',clone:()=>silent},output={stop(){this.stopped=true;}},node=()=>({connect(){},disconnect(){this.disconnected=true;}}),context={createMediaStreamSource:node,createAnalyser:()=>({...node(),getFloatTimeDomainData:data=>data.fill(0)}),createGain:()=>({...node(),gain:{setTargetAtTime(){}}}),createDynamicsCompressor:()=>({...node(),threshold:{},knee:{},ratio:{},attack:{},release:{}}),createMediaStreamDestination:()=>({...node(),stream:new MediaStream([output])})};
 const receiver=createReceiver(context,track);await receiver.play();assert.equal(audios.length,1);assert.equal(audios[0].srcObject.getTracks()[0],silent);assert.equal(silent.enabled,false);assert.equal(audios[0].muted,true);await receiver.play();assert.equal(audios[0].plays,2);
 receiver.destroy();assert.equal(audios[0].paused,true);assert.equal(audios[0].srcObject,null);assert.equal(output.stopped,true);assert.equal(silent.stopped,true);assert.equal(track.stopped,undefined);
});
test('remote automatic gain lifts quiet speakers, softens loud speakers and preserves manual disable',()=>{
 let quiet=1,loud=1;for(let n=0;n<150;n++){quiet=receiveGain(.03,quiet,true);loud=receiveGain(.3,loud,true);}
 assert.ok(Math.abs(quiet-4)<.01);assert.ok(Math.abs(loud-.5)<.01);assert.equal(receiveGain(.01,quiet,false),1);
});
test('remote automatic gain never raises background hiss or pauses and rejects nonfinite levels',()=>{
 for(const rms of [0,.001,NaN,Infinity])assert.equal(receiveGain(rms,1,true),1);
 let boosted=4;for(let i=0;i<50;i++)boosted=receiveGain(.001,boosted,true);assert.ok(Math.abs(boosted-1)<.002);
 for(const rms of [.009,.01,.1,1,100]){let gain=1;for(let i=0;i<100;i++)gain=receiveGain(rms,gain,true);assert.ok(gain>=.5&&gain<=4);}
});
