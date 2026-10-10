import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
import vm from 'node:vm';

const source=ts.createSourceFile('main.tsx',await readFile(new URL('../src/main.tsx',import.meta.url),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let clause;
function visit(node){if(ts.isCaseClause(node)&&node.expression.getText(source)==="'avatar'")clause=node;ts.forEachChild(node,visit);}
visit(source);
assert.ok(clause);
const code=ts.transpileModule(`globalThis.receive=e=>{switch(e.type){${clause.getText(source)}}};`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
function fixture(){
  const timers=new Map(),key='bot-uid:0123456789abcdef0123456789abcdef';let next=0,images={},errors={};
  const context={avatarRequests:{current:new Set([key])},avatarInFlight:{current:new Set([9])},avatarAttempts:{current:new Map()},avatarRetryTimers:{current:new Map()},connectionEpoch:{current:1},setAvatarErrors(fn){errors=fn(errors);},setAvatars(fn){images=fn(images);},setTimeout(fn){const id=++next;timers.set(id,fn);return id;},clearTimeout(id){timers.delete(id);}};
  vm.createContext(context);vm.runInContext(code,context);
  const event={type:'avatar',client:9,uid:'bot-uid',hash:'0123456789abcdef0123456789abcdef',data:null,error:'temporary transport failure'};
  function tick(){const scheduled=[...timers.values()];timers.clear();for(const fn of scheduled)fn();}
  return {context,timers,key,event,tick,images:()=>images};
}
test('transient avatar errors allow bounded retries without permanently caching failure',()=>{
  const f=fixture();
  for(let attempt=0;attempt<3;attempt++){
    f.context.receive(f.event);
    assert.equal(f.timers.size,1);
    f.tick();
    assert.equal(f.context.avatarRequests.current.has(f.key),false);
    f.context.avatarRequests.current.add(f.key);
  }
  f.context.receive(f.event);
  assert.equal(f.timers.size,0,'permanent errors must not cause infinite file transfers');
});
test('avatar retries cannot revive requests from an old server connection',()=>{
  const f=fixture();f.context.receive(f.event);assert.equal(f.timers.size,1);
  f.context.connectionEpoch.current++;f.tick();
  assert.equal(f.context.avatarRequests.current.has(f.key),true);
});
test('a successful avatar response cancels pending retry and keeps the image',()=>{
  const f=fixture();f.context.receive(f.event);
  f.context.receive({...f.event,data:'data:image/png;base64,eA==',error:null});
  assert.equal(f.timers.size,0);assert.equal(f.images()[f.key],'data:image/png;base64,eA==');
});
