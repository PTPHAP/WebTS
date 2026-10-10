import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import vm from 'node:vm';
import ts from 'typescript';
import {renderToStaticMarkup} from 'react-dom/server';
const require=createRequire(import.meta.url);
const code=ts.transpileModule(await readFile(new URL('../src/Stickers.tsx',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX}}).outputText;
const context=vm.createContext({exports:{},require:name=>name==='react'?{...require('react'),useState:()=>[true,()=>{}]}:require(name)});
vm.runInContext(code,context);
const {StickerPicker,Sticker}=context.exports;

test('sticker privacy description follows the actual chat mode',()=>{
  const render=privacy=>renderToStaticMarkup(StickerPicker({emoji:()=>{},privacy}));
  assert.match(render('e2ee'),/表情包随好友私信端到端加密/);
  const server=render('server');
  assert.match(server,/当前私信允许本站解密/);
  assert.doesNotMatch(server,/表情包随好友私信端到端加密/);
  assert.match(render(undefined),/TS 聊天发送兼容客户端的文字表情，使用两段传输加密/);
});

test('unknown sticker IDs never render caller-provided markup',()=>{
  const html=renderToStaticMarkup(Sticker({id:'<img src=x onerror=alert(1)>'}));
  assert.match(html,/未知表情/);
  assert.doesNotMatch(html,/<img|onerror/);
});
