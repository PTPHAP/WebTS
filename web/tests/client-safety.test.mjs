import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
async function load(file){const text=await readFile(new URL(`../src/${file}`,import.meta.url),'utf8');const compiled=ts.transpileModule(text,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);}
const {readAudioSettings,processingLabel}=await load('audio-settings.ts');
const {tsText}=await load('ts-text.ts');
const {imageDimensions,avatarImage}=await load('avatar.ts');
test('malformed persisted audio options revert safely; values are bounded',()=>{
  for(const value of ['{bad','null']){globalThis.localStorage={getItem:()=>value};assert.equal(readAudioSettings().noise,'rnnoise');assert.equal(readAudioSettings().keyboard,true);}
  globalThis.localStorage={getItem:()=>JSON.stringify({noise:'other',gain:100,volume:-9,echo:'yes',autoGain:false})};assert.deepEqual(readAudioSettings(),{noise:'rnnoise',keyboard:true,echo:true,autoGain:false,gain:2,volume:0});
});
test('legacy native preference migrates to local processing; explicit disable is retained',()=>{
  globalThis.localStorage={getItem:()=>JSON.stringify({noise:'browser'})};assert.equal(readAudioSettings().noise,'rnnoise');
  globalThis.localStorage={getItem:()=>JSON.stringify({noise:'off',keyboard:false,echo:false})};const settings=readAudioSettings();assert.equal(settings.noise,'off');assert.equal(settings.keyboard,false);assert.equal(settings.echo,false);
});
test('processing status distinguishes enabled, unavailable and unreported echo cancellation',()=>{
  assert.match(processingLabel('keyboard',{echoCancellation:false,autoGainControl:true}),/键盘.*回声消除.*未启用.*自动增益.*已启用/);
  assert.match(processingLabel('rnnoise',{}),/回声消除.*未报告/);
  assert.equal(processingLabel('listen',{}),'仅收听');
});
test('TS description text never creates active or external image content',()=>{
  const parts=tsText('<script>test</script>[url=javascript:alert(1)]click[/url][url=https://example.com]safe[/url][img]https://example.com/tracker[/img]');
  assert.equal(parts.filter(p=>p.href).length,1);assert.equal(parts.find(p=>p.href).href,'https://example.com/');assert.ok(parts.some(p=>p.text.includes('<script>')));assert.ok(parts.some(p=>p.text.includes('[img]')));assert.equal(tsText('x'.repeat(20000)).map(p=>p.text).join('').length,16384);
});
test('avatar encoded dimensions reject oversized image before invoking decoder',async()=>{
  const header=new Uint8Array(24);header.set([137,80,78,71,13,10,26,10]);new DataView(header.buffer).setUint32(16,100000);new DataView(header.buffer).setUint32(20,100000);
  assert.deepEqual(imageDimensions(header),[100000,100000]);let decoded=false;globalThis.createImageBitmap=()=>{decoded=true;throw new Error('should not decode');};
  await assert.rejects(avatarImage({type:'image/png',size:24,arrayBuffer:async()=>header.buffer}),/尺寸过大/);assert.equal(decoded,false);
  assert.throws(()=>imageDimensions(new TextEncoder().encode('<svg width="1" height="1"/>')),/格式/);
});

const {chatVisible}=await load('chat.ts');
test('private conversation filters both sent and received messages by selected peer',()=>{
 const sentA={scope:'private',from:1,target:2},replyA={scope:'client',from:2,target:1},sentB={scope:'private',from:1,target:3};
 assert.equal(chatVisible(sentA,'private',2),true);assert.equal(chatVisible(replyA,'private',2),true);assert.equal(chatVisible(sentA,'private',3),false);assert.equal(chatVisible(replyA,'private',3),false);assert.equal(chatVisible(sentB,'private',3),true);assert.equal(chatVisible(sentA,'private'),false);
});
