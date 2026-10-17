import {firefox} from '../../node_modules/playwright/index.mjs';
import assert from 'node:assert/strict';
const browser=await firefox.launch({headless:true});
try{
 const page=await browser.newPage();await page.setContent('<button>Start isolated audio check</button>');await page.getByRole('button').click();
 const result=await page.evaluate(async()=>{
  const context=new AudioContext({sampleRate:48000}),source=context.createOscillator(),mute=context.createGain();mute.gain.value=0;source.connect(mute);mute.connect(context.destination);source.start();
  const ready=await Promise.race([context.resume().then(()=>true),new Promise(resolve=>setTimeout(()=>resolve(false),3000))]);const result={ready,state:context.state,sampleRate:context.sampleRate};source.stop();source.disconnect();mute.disconnect();void context.close();return result;
 });
 console.log(JSON.stringify({firefox_audio_backend:result}));assert(result.ready&&result.state==='running','CI needs a working audio backend; do not skip the audio regressions');
}finally{await browser.close();}
