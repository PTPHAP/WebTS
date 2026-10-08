import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createContext,runInContext} from 'node:vm';
import ts from 'typescript';

// Exercise the actual composer and gateway-event handlers, including their
// command-result callback, without duplicating the UI's message logic.
const source=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8');
const file=ts.createSourceFile('main.tsx',source,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let submit,event,command;
function visit(node){
  if(ts.isJsxOpeningElement(node)&&node.tagName.getText(file)==='form'&&node.attributes.properties.some(p=>p.name?.getText(file)==='className'&&p.initializer?.text==='composer')){
    submit=node.attributes.properties.find(p=>p.name?.getText(file)==='onSubmit').initializer.expression;
  }
  if(ts.isNewExpression(node)&&node.expression.getText(file)==='Voice')event=node.arguments[0];
  if(ts.isFunctionDeclaration(node)&&node.name?.text==='command')command=node;
  ts.forEachChild(node,visit);
}
visit(file);assert.ok(submit&&event&&command,'real chat handlers must be found');
const printer=ts.createPrinter();
const code=ts.transpileModule(`${printer.printNode(ts.EmitHint.Unspecified,command,file)}
globalThis.submit=${printer.printNode(ts.EmitHint.Expression,submit,file)};
globalThis.receive=${printer.printNode(ts.EmitHint.Expression,event,file)};`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
function client(scope='channel'){
  let messages=[],draft='same message',sequence=0;const sent=[],notices=[];
  const context=createContext({scope,message:draft,recipient:{id:2},own:{name:'Sender'},state:{own:1},
    crypto:{randomUUID:()=>String(++sequence)},pending:{current:new Map()},voice:{current:{send:value=>sent.push(value)}},
    setChat:update=>{messages=update(messages);},setMessage:value=>{draft=value;},setNotice:value=>notices.push(value),
    moves:{current:{result:()=>false}},avatarAction:{current:''},setAvatarBusy:()=>{},
    sounds:{current:{play:()=>{}}},previousState:{current:{own:1}}});
  runInContext(code,context);
  return {sent,notices,messages:()=>messages,submit:()=>context.submit({preventDefault(){}}),receive:value=>context.receive(value)};
}

for(const scope of ['channel','server'])for(const echoFirst of [true,false]){
  test(`${scope}: one send displays once with ${echoFirst?'echo':'result'} first`,()=>{
    const c=client(scope);c.submit();assert.equal(c.sent.length,1);
    const echo={type:'chat',scope,from:1,name:'Sender',text:'same message'};
    const result={type:'result',id:c.sent[0].id,ok:true};
    for(const value of echoFirst?[echo,result]:[result,echo])c.receive(value);
    assert.equal(c.messages().length,1);assert.equal(c.messages()[0].text,'same message');
  });
}
test('private message remains visible after success without a sender echo',()=>{
  const c=client('private');c.submit();c.receive({type:'result',id:c.sent[0].id,ok:true});
  assert.equal(c.messages().length,1);assert.equal(c.messages()[0].target,2);
  c.receive({type:'chat',scope:'client',from:2,target:1,name:'Peer',text:'reply'});
  assert.equal(c.messages().length,2);
});
for(const echoFirst of [true,false])test(`private sender echo displays once with ${echoFirst?'echo':'result'} first`,()=>{
  const c=client('private');c.submit();
  const echo={type:'chat',scope:'client',from:1,target:2,name:'Sender',text:'same message'};
  const result={type:'result',id:c.sent[0].id,ok:true};
  for(const value of echoFirst?[echo,result]:[result,echo])c.receive(value);
  assert.equal(c.messages().length,1);
  c.receive({type:'chat',scope:'client',from:2,target:1,name:'Peer',text:'same message'});
  assert.equal(c.messages().length,2);
});
test('two intentional identical sends and messages from another member are preserved',()=>{
  const c=client();
  for(let i=0;i<2;i++){c.submit();c.receive({type:'result',id:c.sent[i].id,ok:true});c.receive({type:'chat',scope:'channel',from:1,name:'Sender',text:'same message'});}
  c.receive({type:'chat',scope:'channel',from:2,name:'Peer',text:'same message'});
  assert.equal(c.sent.length,2);assert.equal(c.messages().length,3);
});
test('rejected send is not displayed as delivered',()=>{
  for(const scope of ['channel','server','private']){
    const c=client(scope);c.submit();c.receive({type:'result',id:c.sent[0].id,ok:false,message:'Permission denied'});
    assert.equal(c.messages().length,0);assert.deepEqual(c.notices,['Permission denied']);
  }
});
