import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
import * as React from 'react';

const options={module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX};
const code=ts.transpileModule(await readFile(new URL('../src/Workspace.tsx',import.meta.url),'utf8'),{compilerOptions:options}).outputText;
const helperContext=vm.createContext({exports:{}});
vm.runInContext(ts.transpileModule(await readFile(new URL('../src/workspace-layout.ts',import.meta.url),'utf8'),{compilerOptions:options}).outputText,helperContext);
const helpers=helperContext.exports;
function fixture(compact,editing=true){
  const changes=[],moves=[],children=['channels','chat','members'].map(id=>React.createElement('section',{key:id,'data-id':id}));
  let layout=helpers.readLayout(null),states=0;
  const jsx={jsx:(type,props,key)=>React.createElement(type,{...props,key}),jsxs:(type,props,key)=>React.createElement(type,{...props,key})};
  const context=vm.createContext({exports:{},localStorage:{setItem(){}},matchMedia:()=>({matches:false}),require:name=>name==='react'?{...React,useEffect(){},useRef:v=>({current:v}),useState:initial=>[states++===0?editing:initial,()=>{}]}:name==='react/jsx-runtime'?jsx:name==='./workspace-layout'?helpers:{useAccountPreference:()=>[layout,next=>{layout=next;changes.push(next);} ]}});
  vm.runInContext(code,context);
  const element=context.exports.Workspace({editing,finish(){},mobile:'channels',compact,mobileOrder:['members','chat','channels'],onMobileOrder:v=>moves.push(v),select:()=>{},children});
  const nodes=[];function walk(value){if(Array.isArray(value))value.forEach(walk);else if(React.isValidElement(value)){nodes.push(value);walk(value.props.children);}}walk(element);
  return {nodes,changes,moves,children};
}
test('phone reordering updates only phone preference and keeps all existing panel children mounted',()=>{
  const f=fixture(true);f.nodes.find(n=>n.props['aria-label']==='频道树前移').props.onClick();
  assert.deepEqual([...f.moves[0]],['members','channels','chat']);assert.equal(f.changes.length,0);
  const panes=f.nodes.filter(n=>n.props.className?.startsWith('workspace-pane'));
  assert.deepEqual(panes.map(n=>n.key),['channels','chat','members']);
  for(let i=0;i<3;i++)assert.equal(panes[i].props.children[1].props['data-id'],f.children[i].props['data-id']);
});
test('normal browsing mounts no layout toolbar while every panel stays mounted',()=>{
 for(const compact of [true,false]){
  const f=fixture(compact,false);assert.equal(f.nodes.some(n=>n.props.className==='layout-toolbar'),false);
  assert.equal(f.nodes.filter(n=>n.props.className?.startsWith('workspace-pane')).length,3);
 }
});
test('desktop reordering preserves phone order and offers no phone mode toggle',()=>{
  const f=fixture(false);f.nodes.find(n=>n.props['aria-label']==='频道树后移').props.onClick();
  assert.deepEqual([...f.changes[0].order],['chat','channels','members']);assert.equal(f.moves.length,0);
  assert.equal(f.nodes.some(n=>n.type==='button'&&n.props.children==='紧凑'),false);
  assert.equal(f.nodes.filter(n=>n.props.className?.startsWith('workspace-pane')).length,3);
});
