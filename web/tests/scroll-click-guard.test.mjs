import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/scroll-click-guard.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const {createScrollClickGuard}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
const pointer={pointerId:1,button:0,clientX:20,clientY:20};
function click(guard,detail=1){let blocked=false;guard.onClickCapture({detail,preventDefault(){blocked=true;},stopPropagation(){}});return blocked;}
test('touch or mouse scrolling suppresses only the trailing click; normal tap and keyboard still work',()=>{
 const guard=createScrollClickGuard();guard.onPointerDownCapture(pointer);guard.onPointerMoveCapture({...pointer,clientY:60});guard.onPointerUpCapture(pointer);
 assert.equal(click(guard,0),false);assert.equal(click(guard),true);
 guard.onPointerDownCapture(pointer);guard.onPointerMoveCapture({...pointer,clientY:25});guard.onPointerUpCapture(pointer);assert.equal(click(guard),false);
});
test('native scrolling or cancellation cannot open a row, and a new deliberate tap clears the guard',()=>{
 for(const cancel of [false,true]){
  const guard=createScrollClickGuard();guard.onPointerDownCapture(pointer);
  if(cancel)guard.onPointerCancelCapture(pointer);else{guard.onScrollCapture({});guard.onPointerUpCapture(pointer);}
  assert.equal(click(guard),true);
  guard.onPointerDownCapture(pointer);guard.onPointerUpCapture(pointer);assert.equal(click(guard),false);
 }
});
test('unrelated pointer movement and right click do not cancel normal actions',()=>{
 const guard=createScrollClickGuard();guard.onPointerDownCapture(pointer);guard.onPointerMoveCapture({...pointer,pointerId:2,clientX:100});guard.onPointerUpCapture(pointer);assert.equal(click(guard),false);
 guard.onPointerDownCapture({...pointer,button:2});assert.equal(click(guard),false);
});
test('a second finger does not replace the first gesture or activate a channel after pinch',()=>{
 const guard=createScrollClickGuard();guard.onPointerDownCapture(pointer);
 guard.onPointerDownCapture({...pointer,pointerId:2,isPrimary:false});guard.onPointerUpCapture(pointer);
 assert.equal(click(guard),true);
});
