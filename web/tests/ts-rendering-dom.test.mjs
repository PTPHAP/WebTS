import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import ts from 'typescript';
const require=createRequire(import.meta.url);
const compile=text=>ts.transpileModule(text,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX}}).outputText;
const moduleUrl=code=>`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const textModule=moduleUrl(compile(await readFile(new URL('../src/ts-text.ts',import.meta.url),'utf8')));
let code=compile(await readFile(new URL('../src/ChannelText.tsx',import.meta.url),'utf8'));
code=code.replaceAll("'./ts-text'",JSON.stringify(textModule)).replaceAll('"react/jsx-runtime"',JSON.stringify(pathToFileURL(require.resolve('react/jsx-runtime')).href)).replaceAll("'react'",JSON.stringify(pathToFileURL(require.resolve('react')).href));
const {ChannelText}=await import(moduleUrl(code));
async function component(file){
  let output=compile(await readFile(new URL(`../src/${file}.tsx`,import.meta.url),'utf8'));
  output=output.replaceAll("'./ChannelText'",JSON.stringify(moduleUrl(code))).replaceAll("'./ts-text'",JSON.stringify(textModule)).replaceAll('"react/jsx-runtime"',JSON.stringify(pathToFileURL(require.resolve('react/jsx-runtime')).href));
  return import(moduleUrl(output));
}
const {Legal}=await component('Legal'),{ServerOverview}=await component('ServerOverview');

test('policy placeholders follow branding as escaped text without changing links or enabling markup',()=>{
  const home={site_name:'<script>x</script>[link](https://bad.example)',operator:'运营者',contact:'公开渠道',data_details:'',privacy_policy:'# {{site_name}}\n\n## {{site_name}} 隐私政策\n\n**{{site_name}}** 与 [{{site_name}} 联系](https://example.com)',terms:'{{site_name}} 服务条款'};
  for(const kind of ['privacy','terms']){
    const html=renderToStaticMarkup(createElement(Legal,{home,kind}));
    assert.doesNotMatch(html,/\{\{site_name\}\}|<script|href="https:\/\/bad.example/);
    assert.match(html,/&lt;script&gt;x&lt;\/script&gt;/);
    if(kind==='privacy')assert.match(html,/href="https:\/\/example.com"/);
  }
});
test('server overview uses native metadata and only normalized banner bytes, with safe explicit external links',()=>{
  const state={server:'<img onerror=steal()>',members:[{id:1}],serverInfo:{uid:'fixture',icon:123,banner:'https://images.example.com/x.png',bannerLink:'javascript:steal()',buttonLink:'https://example.com',buttonLabel:'服务器主页',welcome:'[center][b]欢迎[/b][/center]<script>steal()</script>',message:'公告',platform:'Linux',version:'TS3',maxClients:32}};
  const props={state,images:{},loadImage(){},imageLimit:8,back(){}};
  let html=renderToStaticMarkup(createElement(ServerOverview,props));
  assert.doesNotMatch(html,/<img |<script|javascript:|<[^>]+onerror=/);assert.match(html,/显示服务器横幅/);assert.match(html,/text-align:center/);
  html=renderToStaticMarkup(createElement(ServerOverview,{...props,images:{'tsserver:icon:123':{data:'data:image/png;base64,AA=='},'tsserver:banner:https://images.example.com/x.png':{data:'data:image/png;base64,AA=='}}}));
  assert.equal((html.match(/<img /g)||[]).length,2);assert.doesNotMatch(html,/src="https:/);
});
test('real renderer escapes hostile HTML, isolates nested links and validates styles',()=>{
  const html=renderToStaticMarkup(createElement(ChannelText,{text:'<script>alert(1)</script>[center][color=red][b]欢迎[/b][/color][/center][url=javascript:alert(1)]恶意[/url][url=https://example.com][url=https://other.test]链接[/url][/url][color=red;position:fixed]内容[/color]'}));
  assert.match(html,/&lt;script&gt;/);assert.doesNotMatch(html,/<script|javascript:|position:fixed/);assert.match(html,/text-align:center/);assert.match(html,/color:red/);assert.equal((html.match(/<a /g)||[]).length,1);assert.match(html,/noreferrer noopener/);
});
test('native images render only normalized bytes; remote images remain explicit links',()=>{
  const url='ts3image://weixin.jpg?channel=1&path=/';const html=renderToStaticMarkup(createElement(ChannelText,{text:`[img]${url}[/img][img]https://evil.test/tracker[/img]`,images:{[url]:{data:'data:image/png;base64,AA=='}}}));
  assert.match(html,/<img[^>]+data:image\/png/);assert.equal((html.match(/<img /g)||[]).length,1);assert.match(html,/外部站点可见你的 IP/);
  const nested=renderToStaticMarkup(createElement(ChannelText,{text:'[url=https://example.com]'.repeat(24)+'末尾'+'[/url]'.repeat(24)}));assert.equal((nested.match(/<a /g)||[]).length,1);
});

test('actual channel rendering uses the configured per-description picture count',()=>{
  const urls=['a','b','c'].map(name=>`ts3image://${name}.png?channel=1&path=/`);
  const html=renderToStaticMarkup(createElement(ChannelText,{text:urls.map(url=>`[img]${url}[/img]`).join(''),imageLimit:1,images:Object.fromEntries(urls.map(url=>[url,{data:'data:image/png;base64,AA=='}]))}));
  assert.equal((html.match(/<img /g)||[]).length,1);assert.match(html,/图片数量超过站点限制/);
});
