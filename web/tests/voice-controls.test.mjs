import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';

const source=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8');
const ast=ts.createSourceFile('main.tsx',source,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let expression;
function visit(node){if(ts.isVariableDeclaration(node)&&node.name.getText(ast)==='voiceBar')expression=node.initializer.getText(ast);ts.forEachChild(node,visit);}
visit(ast);
assert.ok(expression);
const code=ts.transpileModule(`const voiceBar=${expression};`,{compilerOptions:{jsx:ts.JsxEmit.React,target:ts.ScriptTarget.ES2022}}).outputText+'\nvoiceBar;';
const all=tree=>[tree,...tree.children.filter(v=>v&&typeof v==='object').flatMap(all)];
function render(overrides={}){
 const calls=[];
 const context={React:{createElement:(type,props,...children)=>({type,props:props??{},children:children.flat(Infinity)})},Icon:()=>null,
 connected:true,busy:false,reconnecting:false,away:false,ownSpeaking:false,capturePaused:false,capture:'',audioTransport:'connected',state:{canSpeak:true},own:{name:'测试成员'},transmitting:false,cipher:'AES-256-GCM',processing:'本地 AI',muted:false,deafened:false,mode:'open',pttKey:'KeyV',pttKeyLabel:()=> 'V',disconnect:()=>calls.push('disconnect'),voice:{current:{press:v=>calls.push(v)}},...overrides};
 return {tree:vm.runInContext(code,vm.createContext(context)),calls};
}

test('compact disconnect stays rendered and uses the existing connection cleanup, including reconnect cancellation',()=>{
 for(const values of [{connected:true},{connected:false,reconnecting:true},{connected:false,busy:true}]){
  const {tree,calls}=render(values),button=all(tree).find(v=>v.props.className?.includes('mobile-disconnect'));
  assert.ok(button);assert.equal(button.props['aria-label'],'断开服务器');assert.equal(button.props.disabled,false);
  button.props.onClick();assert.deepEqual(calls,['disconnect']);
 }
 const {tree}=render({connected:false});assert.equal(all(tree).find(v=>v.props.className?.includes('mobile-disconnect')).props.disabled,true);
});

test('speaking states preserve the same control structure and permission checks',()=>{
 let shape;
 for(const values of [{},{transmitting:true,ownSpeaking:true},{capturePaused:true,capture:'正在恢复麦克风'},{state:{canSpeak:false}},{mode:'ptt'}]){
  const {tree,calls}=render(values),buttons=all(tree).filter(v=>v.type==='button'),signature=buttons.map(v=>v.props.className.split(' ')[0]);
  if(shape)assert.deepEqual(signature,shape);else shape=signature;
  const ptt=buttons.find(v=>v.props.className.startsWith('ptt'));
  assert.equal(ptt.props.disabled,values.state?.canSpeak===false);
  if(!ptt.props.disabled){ptt.props.onPointerDown({currentTarget:{setPointerCapture(){}},pointerId:1,button:0});ptt.props.onPointerUp();assert.deepEqual(calls,[true,false]);}
 }
});
