import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {createRequire} from 'node:module';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import ts from 'typescript';
const file=ts.createSourceFile('main.tsx',await readFile(new URL('../src/main.tsx',import.meta.url),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let banner,click;
function visit(node){
  if(ts.isJsxElement(node)){
    const cls=node.openingElement.attributes.properties.find(p=>p.name?.getText(file)==='className')?.initializer?.text;
    if(node.openingElement.tagName.getText(file)==='ChannelPreview')banner=node;
    if(node.openingElement.tagName.getText(file)==='button'&&node.openingElement.attributes.properties.some(p=>p.name?.getText(file)==='onDoubleClick'))click=node.openingElement.attributes.properties.find(p=>p.name?.getText(file)==='onClick').initializer.expression;
  }
  ts.forEachChild(node,visit);
}visit(file);
const compile=expression=>ts.transpileModule(`globalThis.result=(${expression.getText(file)});`,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX}}).outputText;
test('single click previews another channel without joining and central banner uses that preview',()=>{
  const sent=[];let selected;const preview={id:4,name:'Preview channel',topic:'Preview topic',description:'Preview description'};
  const context=vm.createContext({require:createRequire(import.meta.url),exports:{},Icon:()=>null,ChannelPreview:({children})=>createElement('section',{},children),ChannelText:({text})=>createElement('p',{},text),channel:{id:1,description:'Joined description'},inspected:preview,own:{channel:1},imageLimits:{channel_images:8},imageStates:()=>({}),loadTsImage:()=>{},move:()=>{throw Error('preview must not join');},setInspectedChannel:id=>selected=id,setSelected:()=>{},setMobile:()=>{},voice:{current:{send:e=>sent.push(e)}},c:preview});
  vm.runInContext(compile(click),context);context.result();assert.equal(selected,4);assert.equal(sent[0].channel,4);
  vm.runInContext(compile(banner),context);const html=renderToStaticMarkup(context.result);assert.match(html,/Preview description/);assert.match(html,/Preview topic/);assert.doesNotMatch(html,/Joined description/);
});
