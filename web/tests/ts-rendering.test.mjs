import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/ts-text.ts',import.meta.url),'utf8');
const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const module=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('native formatting supports nested multiline alignment, colors and the SDK tag families',()=>{
  assert.equal(typeof module.parseTsText,'function');
  const tree=module.parseTsText('[center][color=#FF8800][b]你好\n世界[/b][/color][/center][s]删除[/s][sup]2[/sup][sub]x[/sub][size=24]大[/size][hr][list=A][*]第一[*]第二[/list][table][tr][th]标题[/th][td]值[/td][/tr][/table]');
  assert.equal(tree[0].tag,'center');assert.equal(tree[0].children[0].tag,'color');assert.equal(tree[0].children[0].children[0].children[0].text,'你好\n世界');
  assert.deepEqual(tree.find(n=>n.tag==='list').children.map(n=>n.tag),['li','li']);assert.equal(tree.find(n=>n.tag==='table').children[0].children[1].tag,'td');
});
test('untrusted markup has bounded depth and tokens and never accepts executable URL or CSS',()=>{
  assert.equal(typeof module.safeHref,'function');
  for(const url of ['javascript:alert(1)','data:text/html,test','file:///etc/passwd','https://user:pass@example.com','https://example.com\n/x'])assert.equal(module.safeHref(url),undefined);
  assert.equal(module.safeColor('url(https://evil.test)'),undefined);assert.equal(module.safeColor('red;position:fixed'),undefined);assert.equal(module.safeColor('#ff8800'),'#ff8800');
  assert.equal(module.safeSize('999999'),undefined);assert.equal(module.safeSize('+2'),20);
  assert.ok(JSON.stringify(module.parseTsText('[b]'.repeat(10000))).length<150000);
});
test('spacers hide the native directive, preserve ordering data and match all alignments',()=>{
  assert.equal(typeof module.channelSpacer,'function');
  assert.deepEqual(module.channelSpacer('[cspacer01]休闲区'),{align:'center',text:'休闲区',repeat:false});
  assert.equal(module.channelSpacer('[rspacer2]管理').align,'right');assert.equal(module.channelSpacer('[*spacer3]━').repeat,true);
  assert.equal(module.channelSpacer('[spacer]---').repeat,true);assert.equal(module.channelSpacer('普通[cspacer]频道'),undefined);
});
