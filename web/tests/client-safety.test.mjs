import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
async function load(file){const text=await readFile(new URL(`../src/${file}`,import.meta.url),'utf8');const compiled=ts.transpileModule(text,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);}
const {readAudioSettings,processingLabel}=await load('audio-settings.ts');
const {tsText}=await load('ts-text.ts');
const {imageDimensions,avatarImage,initialCrop,adjustCrop,croppedImage,loadCropImage,zoomCrop}=await load('avatar.ts');
test('malformed persisted audio options revert safely; values are bounded',()=>{
  for(const value of ['{bad','null']){globalThis.localStorage={getItem:()=>value};assert.equal(readAudioSettings().noise,'rnnoise');assert.equal(readAudioSettings().keyboard,true);}
  globalThis.localStorage={getItem:()=>JSON.stringify({noise:'other',gain:100,volume:-9,echo:'yes',autoGain:false,strength:99,receiveAutoGain:'yes',ducking:-9,typing:0})};assert.deepEqual(readAudioSettings(),{noise:'rnnoise',keyboard:true,voiceOnly:false,echo:true,autoGain:false,gain:2,volume:0,strength:1,receiveAutoGain:true,ducking:0,typing:true});
});
test('legacy native preference migrates to local processing; explicit disable is retained',()=>{
  globalThis.localStorage={getItem:()=>JSON.stringify({noise:'browser'})};assert.equal(readAudioSettings().noise,'rnnoise');
  globalThis.localStorage={getItem:()=>JSON.stringify({noise:'off',keyboard:false,echo:false})};const settings=readAudioSettings();assert.equal(settings.noise,'off');assert.equal(settings.keyboard,false);assert.equal(settings.echo,false);
  globalThis.localStorage={getItem:()=>JSON.stringify({voiceOnly:false})};assert.equal(readAudioSettings().voiceOnly,false);
  globalThis.localStorage={getItem:()=>JSON.stringify({voiceOnly:true,volume:.7})};assert.equal(readAudioSettings().voiceOnly,false);assert.equal(readAudioSettings().volume,.7,'migration must preserve unrelated choices');
  globalThis.localStorage={getItem:()=>JSON.stringify({voiceOnly:true,voiceOnlyRevision:1})};assert.equal(readAudioSettings().voiceOnly,true,'only a new explicit opt-in enables strict filtering');
});
test('processing status distinguishes enabled, unavailable and unreported echo cancellation',()=>{
  assert.match(processingLabel('keyboard',{echoCancellation:false,autoGainControl:true}),/GTCRN.*回声消除.*未启用.*自动增益.*已启用/);
  assert.match(processingLabel('rnnoise',{}),/回声消除.*未报告/);
  assert.equal(processingLabel('listen',{}),'仅收听');
  assert.match(processingLabel('keyboard',{},true),/仅保留人声/);assert.match(processingLabel('blocked',{}),/暂停/);assert.doesNotMatch(processingLabel('off',{},true),/仅保留人声/);
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

test('crop movement and all four corners remain inside the image; avatar stays square',()=>{
  for(const square of [false,true])for(const handle of ['move','nw','ne','sw','se']){
    const crop=initialCrop(800,400,square);
    for(const dx of [-2000,-100,0,100,2000])for(const dy of [-2000,-100,0,100,2000]){
      const next=adjustCrop(crop,800,400,dx,dy,handle,square);
      assert.ok(next.x>=0&&next.y>=0&&next.width>=16&&next.height>=16);
      assert.ok(next.x+next.width<=800&&next.y+next.height<=400);
      if(square)assert.equal(next.width,next.height);
    }
  }
  const next=adjustCrop({x:100,y:100,width:200,height:200},800,400,-50,0,'se',true);
  assert.equal(next.width,150,'keyboard horizontal shrink must also shrink height');
});

test('export uses the selected off-center rectangle and keeps output format/size bounded',()=>{
  const calls=[];let canvas;
  globalThis.document={createElement:()=>canvas={getContext:()=>({drawImage:(...args)=>calls.push(args),fillRect(){}}),toDataURL:type=>`data:${type};base64,eA==`}};
  const image={width:1000,height:800},crop={x:20,y:80,width:100,height:100};
  assert.equal(croppedImage(image,crop,'avatar'),'eA==');
  assert.deepEqual(calls[0].slice(1,5),[20,80,100,100]);assert.equal(canvas.width,256);assert.equal(canvas.height,256);
  assert.equal(croppedImage(image,{x:0,y:0,width:900,height:300},'content'),'data:image/jpeg;base64,eA==');
  assert.equal(canvas.width,900);assert.equal(canvas.height,300);
  for(const bad of [{...crop,x:-1},{...crop,x:950},{...crop,width:NaN},{...crop,width:0}])assert.throws(()=>croppedImage(image,bad,'content'),/裁剪范围/);
  assert.throws(()=>croppedImage(image,{...crop,height:80},'avatar'),/正方形/);
});

test('zoom preserves free aspect ratio and never creates a subpixel crop for tiny or thin images',()=>{
  for(const [width,height] of [[4,4],[1,4096],[4096,1],[800,460]]){
    let crop=initialCrop(width,height,false);
    for(const factor of [1/8,1/8,8,8]){
      crop=zoomCrop(crop,width,height,factor);
      assert.ok(crop.width>=1&&crop.height>=1&&crop.x>=0&&crop.y>=0);
      assert.ok(crop.x+crop.width<=width&&crop.y+crop.height<=height);
      assert.ok(Math.abs(crop.width/crop.height-width/height)<.001);
    }
  }
  assert.equal(zoomCrop(initialCrop(4,4,true),4,4,1/8).width,1);
});

test('decoded image dimensions are checked again and rejected bitmap is released',async()=>{
  const header=new Uint8Array(24);header.set([137,80,78,71]);new DataView(header.buffer).setUint32(16,10);new DataView(header.buffer).setUint32(20,10);
  let closed=false;globalThis.createImageBitmap=async()=>({width:4097,height:10,close:()=>{closed=true;}});
  await assert.rejects(loadCropImage({type:'image/png',size:24,arrayBuffer:async()=>header.buffer},'content'),/尺寸过大/);assert.equal(closed,true);
});

const {chatVisible}=await load('chat.ts');
test('private conversation filters both sent and received messages by selected peer',()=>{
 const sentA={scope:'private',from:1,target:2},replyA={scope:'client',from:2,target:1},sentB={scope:'private',from:1,target:3};
 assert.equal(chatVisible(sentA,'private',2),true);assert.equal(chatVisible(replyA,'private',2),true);assert.equal(chatVisible(sentA,'private',3),false);assert.equal(chatVisible(replyA,'private',3),false);assert.equal(chatVisible(sentB,'private',3),true);assert.equal(chatVisible(sentA,'private'),false);
});
