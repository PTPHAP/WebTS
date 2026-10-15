import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {renderToStaticMarkup} from 'react-dom/server';
import ts from 'typescript';
const source=await readFile(new URL('../src/TsClientName.tsx',import.meta.url),'utf8');
const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext,jsx:ts.JsxEmit.ReactJSX}}).outputText.replaceAll('"react/jsx-runtime"',JSON.stringify(import.meta.resolve('react/jsx-runtime')));
const {TsClientName}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('only an explicit gateway flag shows a badge; names and untrusted HTML stay intact',()=>{
  const marked=renderToStaticMarkup(TsClientName({name:'测试用户',webts:true}));
  assert.match(marked,/测试用户<\/span><small/);assert.match(marked,/>WebTS<\/small>/);
  assert.match(marked,/非身份认证/);
  for(const name of ['测试 [WebTS] 用户','测试 [webts]','普通客户端','<img src=x onerror=alert(1)> [WebTS]']){
    const html=renderToStaticMarkup(TsClientName({name}));
    assert.equal(html.includes('webts-client-tag'),false);
    assert.ok(html.includes('WebTS')||!name.includes('WebTS'));
    assert.equal(html.includes('<img'),false);
  }
});
