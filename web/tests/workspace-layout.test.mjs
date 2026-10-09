import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/workspace-layout.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const {defaultLayout,readLayout,reorderPanels,resizePanels}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('saved layout accepts only three unique panels and bounded widths totaling 100',()=>{
  for(const value of [null,{}, {order:['chat','chat','channels'],widths:defaultLayout.widths},{order:defaultLayout.order,widths:{channels:Infinity,chat:54,members:24}},{order:defaultLayout.order,widths:{channels:22,chat:54,members:25}}])assert.deepEqual(readLayout(value),defaultLayout);
  assert.deepEqual(readLayout(reorderPanels(defaultLayout,'members','channels')).order,['members','channels','chat']);
});
test('moving and resizing preserve every panel, sum and minimum usable width',()=>{
  const moved=reorderPanels(defaultLayout,'channels','members');assert.deepEqual(moved.order,['chat','members','channels']);
  for(const layout of [defaultLayout,moved])for(const index of [0,1])for(const delta of [-1000,-2,2,1000]){
    const next=resizePanels(layout,index,delta);assert.deepEqual(readLayout(next),next);assert.equal(Object.values(next.widths).reduce((a,b)=>a+b),100);
  }
});
