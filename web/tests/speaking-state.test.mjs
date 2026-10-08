import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8'),tree=ts.createSourceFile('main.tsx',source,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);let branch;
function visit(node){if(ts.isCaseClause(node)&&ts.isStringLiteral(node.expression)&&node.expression.text==='speaking')branch=node;ts.forEachChild(node,visit);}visit(tree);
assert.ok(branch,'test the actual application event handler');
const handler=new Function('e','setSpeaking',ts.transpileModule(branch.statements.filter(node=>!ts.isBreakStatement(node)).map(node=>node.getText(tree)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText);
test('real application clears only the ended speaker immediately and still accepts active frames',()=>{
  let state={1:123,2:456};const update=fn=>state=fn(state);
  handler({client:1,enabled:false},update);assert.deepEqual(state,{2:456});
  handler({client:2},update);assert.ok(state[2]>456);assert.equal(state[1],undefined);
  handler({client:2,enabled:false},update);assert.deepEqual(state,{});
});
