import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';

const source=await readFile(new URL('../src/OrdinaryChat.tsx',import.meta.url),'utf8');
const file=ts.createSourceFile('OrdinaryChat.tsx',source,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let send='';const visit=n=>{if(ts.isFunctionDeclaration(n)&&n.name?.text==='send')send=n.getText(file);ts.forEachChild(n,visit);};visit(file);assert.ok(send);
const cryptoSource=await readFile(new URL('../src/friend-crypto.ts',import.meta.url),'utf8');
const c=await import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(cryptoSource,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText).toString('base64')}`);

test('ordinary sending during a receive poll needs no identity, retains only RAM ciphertext and retries the same packet',async()=>{
 const calls=[];let fail=true,pending,status='';
 const context={exports:{},crypto,location:{origin:'https://fixture.example'},sending:{current:false},working:{current:true},alive:{current:true},busy:false,enabled:true,pending:undefined,draft:'synthetic offline message',image:'',account:1,peer:2,days:7,epoch:0,burn:true,seconds:600,sealServerMessage:c.sealServerMessage,setBusy(){},setStatus(v){status=v;},setPending(v){pending=v;context.pending=v;},setDraft(v){context.draft=v;},setImage(v){context.image=v;},receive:async()=>{},refresh:async()=>{},api:async(path,body)=>{assert.equal(path,'/friends/messages');calls.push(structuredClone(body));if(fail)throw Error('synthetic timeout');return {};} };
 const script=ts.transpileModule(send+';exports.send=send;',{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 vm.runInContext(script,vm.createContext(context));await context.exports.send();
 assert.equal(calls.length,1,'receive polling must not swallow the send click');assert.ok(pending);assert.equal(pending.mode,'server');assert.equal(pending.epoch,0);assert.match(status,/同一份密文/);
 const value=await c.openServerMessage(pending.ciphertext,0,{site:'https://fixture.example',id:pending.id,sender:1,recipient:2,burn:true,burn_seconds:600});assert.equal(value.text,context.draft);
 fail=false;await context.exports.send();assert.deepEqual(calls[0],calls[1]);assert.equal(pending,undefined);assert.equal(context.draft,'');assert.match(status,/对象存储/);
 // No browser storage primitive or private identity is available to this send path.
});
