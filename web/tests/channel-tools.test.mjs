import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import ts from 'typescript';
async function module(name){const context=vm.createContext({require:createRequire(import.meta.url),exports:{}});vm.runInContext(ts.transpileModule(await readFile(new URL(`../src/${name}`,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX}}).outputText,context);return context.exports;}
const {channelPatch}=await module('ChannelEditor.tsx');
const {previewHeight}=await module('ChannelPreview.tsx');
const {Legal}=await module('Legal.tsx');
const form=values=>{const f=new FormData();for(const[k,v]of Object.entries(values))f.set(k,String(v));return f;};
const plain=value=>JSON.parse(JSON.stringify(value));
test('channel edits only submit deliberate differences and do not reset codec, topic or password',()=>{
  const target={id:4,name:'Another room',topic:'Topic',description:'Keep description',kind:'permanent',codec:5,quality:7,order:9,maxClients:-1};
  const values={name:target.name,topic:target.topic,description:target.description,kind:target.kind,codec:'music',quality:7,order:9,maxClients:-1,password:''};
  assert.deepEqual(plain(channelPatch(form(values),target)),{});
  assert.deepEqual(plain(channelPatch(form({...values,topic:'Changed'}),target)),{topic:'Changed'});
  assert.deepEqual(plain(channelPatch(form({...values,description:'',changePassword:'on'}),target)),{description:'',password:''});
});
test('unloaded description is omitted and a new empty-description channel has a valid payload',()=>{
  const patch=channelPatch(form({name:'Same',topic:''}),{name:'Same',topic:'',description:undefined});assert.ok(!Object.hasOwn(patch,'description'));
  assert.deepEqual(plain(channelPatch(form({name:'New',description:'',parent:4,codec:'voice',quality:6,kind:'temporary'}))),{name:'New',kind:'temporary',codec:'voice',quality:6,password:'',parent:4,description:''});
});
test('preview height rejects malformed values and bounds both drag extremes',()=>{
  for(const value of [null,{},'200',NaN,Infinity])assert.equal(previewHeight(value),200);
  assert.equal(previewHeight(-10),80);assert.equal(previewHeight(10000),420);assert.equal(previewHeight(250),250);
});
test('custom policies and operator details remain text instead of executing HTML',()=>{
  const html=renderToStaticMarkup(createElement(Legal,{kind:'privacy',home:{site_name:'Test',operator:'<script>operator</script>',contact:'private requests',data_details:'<img src=x>',privacy_policy:'## Privacy\n\n<img src=x onerror=alert(1)>'}}));
  assert.ok(!html.includes('<script>'));assert.ok(!html.includes('<img'));assert.match(html,/&lt;img/);assert.match(html,/Privacy/);
});
