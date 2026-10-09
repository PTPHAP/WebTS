import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {runInNewContext} from 'node:vm';
import ts from 'typescript';
const text=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8');
const file=ts.createSourceFile('main.tsx',text,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let tree;function visit(node){if(ts.isFunctionDeclaration(node)&&node.name?.text==='tree')tree=node;ts.forEachChild(node,visit);}visit(file);
const code=ts.transpileModule(ts.createPrinter().printNode(ts.EmitHint.Unspecified,tree,file),{compilerOptions:{target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.React,jsxFactory:'h'}}).outputText;
let channelSiblings;
try{const source=await readFile(new URL('../src/channel-order.ts',import.meta.url),'utf8');const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;({channelSiblings}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`));}catch(e){if(e.code!=='ENOENT')throw e;}
const spacerSource=await readFile(new URL('../src/ts-text.ts',import.meta.url),'utf8');
const {channelSpacer}=await import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(spacerSource,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText).toString('base64')}`);
function render(channels,parent=0){return runInNewContext(`${code};tree(${parent})`,{state:{channels,members:[]},own:{},inspectedChannel:undefined,Icon:()=>{},channelSiblings,channelSpacer,h:(tag,props,...children)=>({tag,props,children})});}
const channels=[{id:10,parent:0,order:30},{id:20,parent:0,order:10},{id:30,parent:0,order:0},{id:4,parent:30,order:90},{id:90,parent:30,order:0}];
test('actual channel tree follows predecessor IDs instead of numeric order',()=>{assert.deepEqual(Array.from(render(channels),n=>n.props.key),[30,10,20]);});
test('nested siblings keep their own TS order and follow live repositioning',()=>{
  assert.deepEqual(Array.from(render(channels,30),n=>n.props.key),[90,4]);
  const moved=channels.map(c=>({...c,order:c.id===10?0:c.id===30?20:c.order}));
  assert.deepEqual(Array.from(render(moved),n=>n.props.key),[10,20,30]);
});
test('missing predecessors or cycles never hide channels or loop forever',()=>{
  const broken=[{id:7,parent:0,order:99},{id:2,parent:0,order:7},{id:3,parent:0,order:4},{id:4,parent:0,order:3}];
  const result=Array.from(render(broken),n=>n.props.key);assert.equal(result.length,4);assert.equal(new Set(result).size,4);assert.deepEqual(result.slice(0,2),[7,2]);
});
