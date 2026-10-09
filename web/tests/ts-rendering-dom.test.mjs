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
test('real renderer escapes hostile HTML, isolates nested links and validates styles',()=>{
  const html=renderToStaticMarkup(createElement(ChannelText,{text:'<script>alert(1)</script>[center][color=red][b]欢迎[/b][/color][/center][url=javascript:alert(1)]恶意[/url][url=https://example.com][url=https://other.test]链接[/url][/url][color=red;position:fixed]内容[/color]'}));
  assert.match(html,/&lt;script&gt;/);assert.doesNotMatch(html,/<script|javascript:|position:fixed/);assert.match(html,/text-align:center/);assert.match(html,/color:red/);assert.equal((html.match(/<a /g)||[]).length,1);assert.match(html,/noreferrer noopener/);
});
test('native images render only normalized bytes; remote images remain explicit links',()=>{
  const url='ts3image://weixin.jpg?channel=1&path=/';const html=renderToStaticMarkup(createElement(ChannelText,{text:`[img]${url}[/img][img]https://evil.test/tracker[/img]`,images:{[url]:{data:'data:image/png;base64,AA=='}}}));
  assert.match(html,/<img[^>]+data:image\/png/);assert.equal((html.match(/<img /g)||[]).length,1);assert.match(html,/外部站点可见你的 IP/);
  const nested=renderToStaticMarkup(createElement(ChannelText,{text:'[url=https://example.com]'.repeat(24)+'末尾'+'[/url]'.repeat(24)}));assert.equal((nested.match(/<a /g)||[]).length,1);
});
