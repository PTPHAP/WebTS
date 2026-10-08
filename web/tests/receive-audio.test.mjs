import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/receive-audio.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {receiveGain,createReceiver}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));

test('remote RTC decoding starts on the original track before Web Audio and resumes without duplicate sound',async()=>{
 const audios=[];globalThis.Audio=class{constructor(){audios.push(this);}play(){this.plays=(this.plays??0)+1;return Promise.resolve();}pause(){this.paused=true;}};
 globalThis.MediaStream=class{constructor(tracks){this.tracks=tracks;}getTracks(){return this.tracks;}};
 const track={id:'rtc'},output={stop(){this.stopped=true;}},node=()=>({connect(){},disconnect(){this.disconnected=true;}}),context={createMediaStreamSource:node,createAnalyser:()=>({...node(),getFloatTimeDomainData:data=>data.fill(0)}),createGain:()=>({...node(),gain:{setTargetAtTime(){}}}),createDynamicsCompressor:()=>({...node(),threshold:{},knee:{},ratio:{},attack:{},release:{}}),createMediaStreamDestination:()=>({...node(),stream:new MediaStream([output])})};
 const receiver=createReceiver(context,track);await receiver.play();assert.equal(audios.length,1);assert.equal(audios[0].srcObject.getTracks()[0],track);assert.equal(audios[0].volume,0);assert.notEqual(audios[0].muted,true);await receiver.play();assert.equal(audios[0].plays,2);
 receiver.destroy();assert.equal(audios[0].paused,true);assert.equal(audios[0].srcObject,null);assert.equal(output.stopped,true);assert.equal(track.stopped,undefined);
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
