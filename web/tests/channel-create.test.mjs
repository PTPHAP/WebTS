import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
const file=ts.createSourceFile('main.tsx',await readFile(new URL('../src/main.tsx',import.meta.url),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let submit,event,command;
function visit(n){
  if(ts.isJsxOpeningElement(n)&&n.tagName.getText(file)==='form'){
    const handler=n.attributes.properties.find(p=>p.name?.getText(file)==='onSubmit')?.initializer?.expression;
    if(handler?.getText(file).includes('payload.kind'))submit=handler;
  }
  if(ts.isNewExpression(n)&&n.expression.getText(file)==='Voice')event=n.arguments[0];
  if(ts.isFunctionDeclaration(n)&&n.name?.text==='command')command=n;
  ts.forEachChild(n,visit);
}visit(file);assert.ok(submit&&event&&command);
const code=ts.transpileModule(`${command.getText(file)};globalThis.submit=${submit.getText(file)};globalThis.receive=${event.getText(file)};`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
function fixture(){const sent=[];let modal='channel_create',error='';let sequence=0;const context=vm.createContext({connected:true,channelSaving:false,channelAction:{current:''},modal,channel:{id:1},crypto:{randomUUID:()=>String(++sequence)},pending:{current:new Map()},voice:{current:{send:v=>sent.push(v)}},moves:{current:{result:()=>false}},avatarAction:{current:''},setAvatarBusy:()=>{},setNotice:()=>{},setChannelError:v=>error=v,setChannelSaving:v=>context.channelSaving=v,setModal:v=>modal=typeof v==='function'?v(modal):v,FormData:class{get(key){return {name:'Music room',parent:'0',description:'Draft stays',password:'',kind:'temporary',codec:'music',quality:'10',topic:'Music'}[key];}}});vm.runInContext(code,context);return {context,sent,modal:()=>modal,error:()=>error};}
test('channel creation blocks duplicate submits and keeps the draft after TS permission rejection',()=>{
  const f=fixture();f.context.submit({preventDefault(){},currentTarget:{}});f.context.submit({preventDefault(){},currentTarget:{}});assert.equal(f.sent.length,1);assert.equal(f.modal(),'channel_create');assert.equal(f.sent[0].codec,'music');assert.equal(f.sent[0].quality,10);
  f.context.receive({type:'result',id:f.sent[0].id,ok:false,message:'当前 TeamSpeak 身份权限不足'});assert.equal(f.modal(),'channel_create');assert.match(f.error(),/权限不足/);assert.equal(f.context.channelSaving,false);
  f.context.submit({preventDefault(){},currentTarget:{}});f.context.receive({type:'result',id:f.sent[1].id,ok:true});assert.equal(f.modal(),'');
});
