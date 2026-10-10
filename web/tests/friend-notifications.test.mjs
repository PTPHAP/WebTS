import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/friend-notifications.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {friendEvents}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('new messages use a sequence cursor, even when reading leaves unread count unchanged',()=>{
  assert.deepEqual(friendEvents({latest:5,relations:[]},{latest:6,relations:[]},1),[{kind:'message'}]);
  assert.deepEqual(friendEvents(undefined,{latest:6,relations:[]},1),[]);
  assert.deepEqual(friendEvents({latest:6,relations:[]},{latest:6,relations:[]},1),[]);
});
test('incoming requests and accepted outgoing requests notify once, profile edits and own acceptance do not',()=>{
  const request={id:2,status:0,requester:2},outgoing={id:3,status:0,requester:1};
  const base={latest:0,relations:[]},pending={latest:0,relations:[request,outgoing]};
  assert.deepEqual(friendEvents(base,pending,1),[{kind:'request',peer:2}]);
  assert.deepEqual(friendEvents(pending,{latest:0,relations:[{...request,status:1},{...outgoing,status:1}]},1),[{kind:'accepted',peer:3}]);
  assert.deepEqual(friendEvents(pending,{...pending},1),[]);
  assert.deepEqual(friendEvents(pending,{latest:0,relations:[{...request,status:2}]},1),[]);
});
