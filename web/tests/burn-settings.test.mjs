import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';

const cryptoSource=await readFile(new URL('../src/friend-crypto.ts',import.meta.url),'utf8');
const {burnDurations}=await import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(cryptoSource,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText).toString('base64')}`);
const source=await readFile(new URL('../src/BurnSettings.tsx',import.meta.url),'utf8');
const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.React}}).outputText;

function render(props){
 const panel={open:true,querySelector(){return {focus(){panel.focused=true;}};}},context={exports:{},React:{createElement:(type,props,...children)=>({type,props:props??{},children:children.flat(Infinity)})},require:name=>name==='react'?{useRef:()=>({current:panel})}:{burnDurations}};
 vm.runInContext(code,vm.createContext(context));
 return {tree:context.exports.BurnSettings(props),panel};
}
const all=tree=>[tree,...tree.children.filter(v=>v&&typeof v==='object').flatMap(all)];

test('all nine choices select duration without silently enabling burn; Escape closes only this sheet',()=>{
 let enabled=false,seconds=60;
 const {tree,panel}=render({enabled,seconds,onEnabled:v=>enabled=v,onSeconds:v=>seconds=v});
 const buttons=all(tree).filter(v=>v.type==='button'&&'aria-pressed' in v.props);
 assert.equal(buttons.length,9);assert.equal(buttons[0].props['aria-pressed'],true);
 buttons.at(-1).props.onClick();assert.equal(seconds,604800);assert.equal(enabled,false);
 all(tree).find(v=>v.type==='input').props.onChange({target:{checked:true}});assert.equal(enabled,true);
 let prevented=false,stopped=false;tree.props.onKeyDown({key:'Escape',preventDefault(){prevented=true;},stopPropagation(){stopped=true;}});
 assert.equal(panel.open,false);assert.equal(panel.focused,true);assert.equal(prevented,true);assert.equal(stopped,true);
});

test('a pending ordinary ciphertext locks policy controls but still allows dismissing the sheet',()=>{
 const {tree,panel}=render({enabled:true,seconds:600,disabled:true,onEnabled(){},onSeconds(){}});
 for(const node of all(tree).filter(v=>v.type==='input'||v.type==='button'&&'aria-pressed' in v.props))assert.equal(node.props.disabled,true);
 const done=all(tree).find(v=>v.type==='button'&&v.children.includes('完成'));assert.equal(done.props.disabled,undefined);done.props.onClick();assert.equal(panel.open,false);
});
