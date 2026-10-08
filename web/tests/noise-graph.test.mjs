import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
let nodes=[],failedModel='',duringKeyboard;
globalThis.AudioWorkletNode=class {
  messages=[];port={onmessage:null,postMessage:value=>{this.messages.push(value);if(value==='destroy')this.destroyed=true;}};
  constructor(_,id,options){this.id=id;this.options=options;nodes.push(this);queueMicrotask(()=>{if(id.endsWith('/gtcrn'))duringKeyboard?.();this.port.onmessage?.({data:{type:id.endsWith('/'+failedModel)?'error':'ready'}});});}
  connect(target){this.target=target;}disconnect(){this.disconnected=true;}
};
const source=await readFile(new URL('../src/noise.ts',import.meta.url),'utf8');
const packageUrl=new URL('../node_modules/@sapphi-red/web-noise-suppressor/dist/index.js',import.meta.url).href;
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText.replace("import('@sapphi-red/web-noise-suppressor')",`import(${JSON.stringify(packageUrl)})`);
const {createNoise}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
function environment(){nodes=[];failedModel='';duringKeyboard=undefined;globalThis.window={setTimeout,clearTimeout};globalThis.fetch=async()=>({ok:true,arrayBuffer:async()=>new ArrayBuffer(8)});const input={connect(target){this.target=target;},disconnect(){this.disconnected=true;}};return {sampleRate:48000,state:'running',audioWorklet:{addModule:async()=>{}},createGain:()=>input,input};}
test('keyboard graph connects GTCRN before RNNoise and runtime GTCRN failure preserves RNNoise',async()=>{
  const context=environment(),processor=await createNoise(context,true,true);assert.equal(processor.mode,'keyboard');assert.equal(nodes[0].options.processorOptions.voiceOnly,true);assert.equal(context.input.target,nodes[1]);assert.equal(nodes[1].target,nodes[0]);assert.equal(processor.output,nodes[0]);
  let notices=0;processor.onchange=()=>notices++;nodes[1].onprocessorerror();assert.equal(processor.mode,'rnnoise');assert.equal(context.input.target,nodes[0]);assert.ok(nodes[1].destroyed);assert.equal(notices,1);
  assert.deepEqual(nodes[0].messages,[{type:'preserve',enabled:true},{type:'preserve',enabled:false}],'GTCRN output is never denoised twice; fallback restores RNNoise filtering');
  let errors=0;processor.onerror=()=>errors++;nodes[0].onprocessorerror();assert.equal(errors,1);processor.destroy();assert.ok(nodes.every(n=>n.destroyed&&n.disconnected));
});
test('unavailable SIMD keyboard processor retains local RNNoise; primary initialization failure rejects',async()=>{
  let context=environment();failedModel='gtcrn';let processor=await createNoise(context,true);assert.equal(processor.mode,'rnnoise');assert.match(processor.warning,/继续使用本地/);assert.equal(context.input.target,nodes[0]);assert.ok(nodes[1].destroyed);processor.destroy();
  context=environment();failedModel='rnnoise';await assert.rejects(createNoise(context,true),/初始化失败/);assert.ok(nodes[0].destroyed);
});
test('primary processor dying during keyboard loading never returns a silently broken graph',async()=>{
  const context=environment();duringKeyboard=()=>nodes[0].onprocessorerror();await assert.rejects(createNoise(context,true),/处理器停止/);assert.ok(nodes.every(n=>n.destroyed&&n.disconnected));
});
test('non-48k context is refused; a closed connection cleans both initialized processors',async()=>{
  let context=environment();context.sampleRate=44100;await assert.rejects(createNoise(context,true),/48 kHz/);assert.equal(nodes.length,0);
  context=environment();duringKeyboard=()=>context.state='closed';await assert.rejects(createNoise(context,true),/连接已关闭/);assert.ok(nodes.every(n=>n.destroyed));
});


test('model speech transitions reach the consumer and cleanup detaches port messages',async()=>{
  const processor=await createNoise(environment(),true,true),states=[];processor.onspeech=enabled=>states.push(enabled);
  nodes[0].port.onmessage({data:{type:'speech',enabled:true}});nodes[0].port.onmessage({data:{type:'speech',enabled:false}});
  assert.deepEqual(states,[true,false]);assert.equal(processor.speaking,false);processor.destroy();assert.equal(nodes[0].port.onmessage,null);
});
