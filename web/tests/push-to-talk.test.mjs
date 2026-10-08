import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {runInNewContext} from 'node:vm';
import ts from 'typescript';
const text=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8');
const file=ts.createSourceFile('main.tsx',text,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let effect;function visit(node){if(ts.isCallExpression(node)&&node.expression.getText(file)==='useEffect'&&node.arguments[0]?.getText(file).includes('voice.current?.press'))effect=node.arguments[0];ts.forEachChild(node,visit);}visit(file);assert.ok(effect);
const code=ts.transpileModule(`(${ts.createPrinter().printNode(ts.EmitHint.Expression,effect,file)})()`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
let helpers={};
try{const source=await readFile(new URL('../src/push-to-talk.ts',import.meta.url),'utf8');const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;helpers=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);}catch(e){if(e.code!=='ENOENT')throw e;}
class Element {constructor(editable=false){this.editable=editable;}closest(){return this.editable?this:null;}}
class Input extends Element {constructor(){super(true);}}
globalThis.Element=Element;
function fixture(pttKey='KeyV',connected=true){
  const listeners=new Map(),calls=[];
  const events={addEventListener:(kind,fn)=>listeners.set(kind,fn),removeEventListener:kind=>listeners.delete(kind)};
  const document={...events,hidden:false,hasFocus:()=>true};
  const cleanup=runInNewContext(code,{...helpers,window:events,document,mode:'ptt',connected,pttKey,voice:{current:{press:enabled=>calls.push(enabled)}},HTMLInputElement:Input,HTMLTextAreaElement:class{},HTMLSelectElement:class{},HTMLButtonElement:class{}});
  const emit=(type,options={})=>listeners.get(type)?.({type,code:pttKey,target:new Element(),preventDefault(){},...options});
  return {calls,listeners,cleanup,emit};
}
test('default V presses and releases once; repeat keydown does not retrigger',()=>{const f=fixture();f.emit('keydown');f.emit('keydown',{repeat:true});f.emit('keyup');assert.deepEqual(f.calls,[true,false]);});
test('custom key replaces V and editable/composing/shortcut input never transmits',()=>{
  const f=fixture('KeyB');f.emit('keydown',{code:'KeyV'});f.emit('keydown',{target:new Element(true)});f.emit('keydown',{isComposing:true});f.emit('keydown',{ctrlKey:true});assert.equal(f.calls.length,0);f.emit('keydown');f.emit('keyup');assert.deepEqual(f.calls,[true,false]);
});
test('release still stops after focus moves into a text field',()=>{const f=fixture('Space');f.emit('keydown');f.emit('keyup',{target:new Input()});assert.deepEqual(f.calls,[true,false]);});
test('blur, background and rebinding cleanup release speech and remove listeners',()=>{
  const f=fixture();f.emit('keydown');f.emit('blur');assert.equal(f.calls.at(-1),false);f.emit('keydown');f.emit('visibilitychange');assert.equal(f.calls.at(-1),false);f.emit('keydown');f.cleanup();assert.equal(f.calls.at(-1),false);assert.equal(f.listeners.size,0);
});
test('disconnected page cannot transmit',()=>{const f=fixture('KeyV',false);f.emit('keydown');assert.ok(!f.calls.includes(true));});
test('missing or invalid saved key uses V; valid custom key and labels persist',()=>{
  for(const saved of [null,'invalid','KeyB','ShiftLeft']){globalThis.localStorage={getItem:()=>saved};assert.equal(helpers.readPttKey(),saved==='KeyB'||saved==='ShiftLeft'?saved:'KeyV');}
  assert.equal(helpers.pttKeyLabel('KeyV'),'V');assert.equal(helpers.pttKeyLabel('Space'),'空格');
});
