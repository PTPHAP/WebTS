import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {runInNewContext} from 'node:vm';
import ts from 'typescript';
const text=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8');
const file=ts.createSourceFile('main.tsx',text,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let tree;function visit(node){if(ts.isFunctionDeclaration(node)&&node.name?.text==='tree')tree=node;ts.forEachChild(node,visit);}visit(file);
const code=ts.transpileModule(ts.createPrinter().printNode(ts.EmitHint.Unspecified,tree,file),{compilerOptions:{target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.React,jsxFactory:'h'}}).outputText;
let channelSiblings;
try{const source=await readFile(new URL('../src/channel-order.ts',import.meta.url),'utf8');const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;({channelSiblings}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`));}catch(e){if(e.code!=='ENOENT')throw e;}
const spacerSource=await readFile(new URL('../src/ts-text.ts',import.meta.url),'utf8');
const {channelSpacer}=await import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(spacerSource,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText).toString('base64')}`);
const foldsSource=await readFile(new URL('../src/channel-folds.ts',import.meta.url),'utf8');
const {foldedChannelMembers}=await import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(foldsSource,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText).toString('base64')}`);
const indexCode=ts.transpileModule(await readFile(new URL('../src/channel-index.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const indexContext={exports:{},require:()=>({channelSiblings})};runInNewContext(indexCode,indexContext);const {indexChannels}=indexContext.exports;
function render(channels,parent=0,folded=[],members=[],extra={}){return runInNewContext(`${code};tree(${parent})`,{state:{channels,members},channelIndex:indexChannels(channels,members),hiddenMembers:new Map(folded.map(id=>[id,foldedChannelMembers(channels,members,id)])),own:{},inspectedChannel:undefined,selected:undefined,avatars:{},speaking:{},Avatar:()=>{},foldScope:'serverA',foldedChannels:new Set(folded),foldedChannelMembers,Icon:()=>{},channelSiblings,channelSpacer,h:(tag,props,...children)=>({tag,props,children}),...extra});}
const channels=[{id:10,parent:0,order:30},{id:20,parent:0,order:10},{id:30,parent:0,order:0},{id:4,parent:30,order:90},{id:90,parent:30,order:0}];
test('actual channel tree follows predecessor IDs instead of numeric order',()=>{assert.deepEqual(Array.from(render(channels),n=>n.props.key),[30,10,20]);});
test('nested siblings keep their own TS order and follow live repositioning',()=>{
  assert.deepEqual(Array.from(render(channels,30),n=>n.props.key),[90,4]);
  const moved=channels.map(c=>({...c,order:c.id===10?0:c.id===30?20:c.order}));
  assert.deepEqual(Array.from(render(moved),n=>n.props.key),[10,20,30]);
});
test('missing predecessors or cycles never hide channels or loop forever',()=>{
  const broken=[{id:7,parent:0,order:99},{id:2,parent:0,order:7},{id:3,parent:0,order:4},{id:4,parent:0,order:3}];
  const result=Array.from(render(broken),n=>n.props.key);assert.equal(result.length,4);assert.equal(new Set(result).size,4);assert.deepEqual(result.slice(0,2),[7,2]);
});
function buttons(nodes){const out=[];function visit(n){if(Array.isArray(n))return n.forEach(visit);if(!n||typeof n!=='object')return;if(n.tag==='button')out.push(n);n.children?.forEach(visit);}visit(nodes);return out;}
test('folding a channel hides both its members and descendants while leaving siblings usable',()=>{
  const members=[{id:1,channel:30,name:'Native member'}];
  const open=buttons(render(channels,0,[],members));
  const closed=buttons(render(channels,0,[30],members));
  assert.ok(open.some(n=>n.props.className.includes('member-row')));
  assert.equal(closed.filter(n=>n.props.className.includes('member-row')).length,0);
  assert.equal(closed.filter(n=>n.props.className.split(' ').includes('channel')).length,3);
  const toggle=closed.find(n=>n.props.className==='channel-fold');
  assert.equal(toggle.props['aria-expanded'],false);
});

test('phone channel selection stays in the tree and fold button never joins a channel',()=>{
  const calls=[];
  const controls=buttons(render(channels,0,[],[],{phone:true,channelMenu:(c,x,y)=>calls.push(['menu',c.id,x,y]),setMobile:()=>assert.fail('phone preview must not replace the tree'),toggleChannel:id=>calls.push(['fold',id]),move:()=>assert.fail('a fold must not join a channel')}));
  controls.find(n=>n.props.className.split(' ').includes('channel')).props.onClick({currentTarget:{getBoundingClientRect:()=>({left:10,bottom:60})}});
  controls.find(n=>n.props.className==='channel-fold').props.onClick();
  assert.deepEqual(calls,[['menu',30,10,60],['fold',30]]);
});
test('collapsed channel row shows at most three supplied avatars and an overflow count',()=>{
 const members=Array.from({length:5},(_,id)=>({id,channel:id===4?20:4,name:`Member ${id}`}));
 const rows=buttons(render(channels,0,[30],members));
 const row=rows.find(n=>n.props.className.split(' ').includes('channel'));
 const stack=row.children.find(n=>n?.props?.className==='folded-members');
 assert.equal(stack.props['aria-label'],'已收起 4 位成员');
 assert.equal(stack.children[0].length,3);
 assert.deepEqual(Array.from(stack.children[0],n=>n.props.member.id),[0,1,2]);
 assert.equal(stack.children[1].children.join(''),'+1');
});

test('channel names provide the phone menu without duplicate three-dot buttons',()=>{
 const controls=buttons(render(channels,0,[],[],{phone:true}));
 assert.equal(controls.filter(n=>n.props.className==='channel-actions').length,0);
});
