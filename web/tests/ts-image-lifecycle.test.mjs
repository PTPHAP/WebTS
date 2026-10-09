import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';

const source=ts.createSourceFile('main.tsx',await readFile(new URL('../src/main.tsx',import.meta.url),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
const app=source.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name.text==='App');
const names=['applyImageLimits','resetTsImages','disconnect','connection','loadTsImage','pumpTsImages'];
const code=ts.transpileModule(app.body.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name.text)).map(n=>n.getText(source)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
const policyCode=ts.transpileModule(await readFile(new URL('../src/image-limits.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {defaultImageLimits,readImageLimits}=await import(`data:text/javascript;base64,${Buffer.from(policyCode).toString('base64')}`);
function fixture(){
  const url='ts3image://same.png?channel=1&path=/';const key=`1:${url}`;const sent=[];
  const context={readImageLimits,imagePolicy:{current:defaultImageLimits},channelAction:{current:''},connectionEpoch:{current:1},tsImageCache:{current:{[key]:{data:'prior-server-private-image'}}},tsImagePending:{current:new Map([['old',{key,epoch:1}]])},tsImageQueue:{current:[{id:'queued',key,url,source:1,epoch:1}]},tsImageCounter:{current:2},connected:true,voice:{current:{close(){},send:e=>sent.push(e),configure(){},connect:async()=>{}}},previousState:{current:null},sounds:{current:null},moves:{current:null},avatarRequests:{current:new Set()},avatarInFlight:{current:new Set()},pending:{current:new Map()},audio:{},server:'test',identity:'new-account',page:{current:'page'},input:'',navigator:{},FormData:class{get(){return 'on';}}};
  for(const name of ['ChannelSaving','ChannelError','ImageLimits','TsImages','Reconnecting','Quality','TsRtt','PasswordTarget','Modal','Speaking','Busy','Avatars','AvatarErrors','Chat','Volumes','Scope','InspectedChannel','Selected','AvatarBusy','Processing','Connected','State','Status','Cipher','Transmitting','Notice'])context[`set${name}`]=()=>{};
  vm.createContext(context);vm.runInContext(code,context);return {context,key,url,sent};
}
for(const boundary of ['manual disconnect/account change','new server connection'])test(`channel images are cleared at ${boundary}`,async()=>{
  const {context,key,url,sent}=fixture();
  if(boundary==='new server connection')await context.connection({preventDefault(){},currentTarget:{}});else context.disconnect();
  assert.equal(Object.keys(context.tsImageCache.current).length,0);assert.equal(context.tsImagePending.current.size,0);assert.equal(context.tsImageQueue.current.length,0);
  context.loadTsImage(1,url);assert.equal(sent.length,1);assert.equal(sent[0].type,'ts_image_get');assert.equal(context.tsImageCache.current[key].data,undefined);assert.equal(context.tsImagePending.current.get(sent[0].id).epoch,2);
});

test('hot image policy clears stale responses and enforces the new cache bound',()=>{
  const {context,url,sent}=fixture();context.applyImageLimits({...defaultImageLimits,channel_cache:1});assert.equal(context.tsImagePending.current.size,0);assert.equal(context.tsImageQueue.current.length,0);context.loadTsImage(1,url);context.loadTsImage(1,'ts3image://second.png?channel=1&path=/');assert.equal(sent.length,1);assert.equal(Object.keys(context.tsImageCache.current).length,1);
});
