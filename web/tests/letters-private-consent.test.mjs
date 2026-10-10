import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {createRequire} from 'node:module';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import ts from 'typescript';
const require=createRequire(import.meta.url);
async function module(file,mocks={}){const source=await readFile(new URL(`../src/${file}`,import.meta.url),'utf8');const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX}}).outputText;const context=vm.createContext({exports:{},crypto:globalThis.crypto,localStorage:{getItem:()=>null},require:name=>mocks[name]??require(name)});vm.runInContext(code,context);return context.exports;}
const inbox=await module('Inbox.tsx',{'./api':{api(){throw Error('SSR must not fetch');}}});
const privateChat=await module('PrivateChat.tsx',{'./Stickers':await module('Stickers.tsx')});
const preview=await module('ChannelPreview.tsx');
test('notice external images require explicit navigation and inline sanitized images stay local',()=>{
  const html=inbox.noticeDisplayHtml('<p>内容</p><img src="https://images.example/a.jpg?a=1&amp;b=2" alt="通知"><img src="data:image/jpeg;base64,c2FmZQ==" alt="本地">');assert.doesNotMatch(html,/<img src="https:/);assert.match(html,/查看外部图片/);assert.match(html,/rel="noopener noreferrer nofollow"/);assert.match(html,/data:image\/jpeg/);assert.equal(inbox.noticeDisplayHtml(html),html);
});
test('initial inbox is accessible and no notifications are falsely unread',()=>{const html=renderToStaticMarkup(createElement(inbox.Inbox,{onNew(){throw Error('initial render must not chime');}}));assert.match(html,/aria-label="站点信箱"/);assert.doesNotMatch(html,/inbox-badge/);});
test('private conversation owns its log and composer, retaining offline recipient without sending',()=>{
  const peer={id:2,uid:'peer',name:'Peer'};const props={members:[],peer,draft:'Private draft',setDraft(){},choose(){},send(){return false;},count:0};const html=renderToStaticMarkup(createElement(privateChat.PrivateChat,props));assert.match(html,/aria-label="独立私聊会话"/);assert.match(html,/aria-label="私聊消息"/);assert.match(html,/class="private-composer composer"/);assert.match(html,/已离线/);assert.match(html,/disabled="" value="Private draft"/);assert.doesNotMatch(html,/频道介绍/);
});
test('preview fixed header is a sibling before the scrollable body',()=>{const html=renderToStaticMarkup(createElement(preview.ChannelPreview,{header:createElement('strong',{},'频道标题')},createElement('p',{},'频道正文')));assert.match(html,/<header class="channel-preview-header"><strong>频道标题<\/strong><\/header><div class="channel-preview-content"><p>频道正文/);});
const source=await readFile(new URL('../src/main.tsx',import.meta.url),'utf8'),file=ts.createSourceFile('main.tsx',source,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let consent,authenticate;function visit(n){if(ts.isJsxElement(n)&&n.openingElement.tagName.getText(file)==='label'&&n.openingElement.attributes.properties.some(p=>p.name?.getText(file)==='className'&&p.initializer?.text==='checkbox auth-consent'))consent=n;if(ts.isFunctionDeclaration(n)&&n.name?.text==='authenticate')authenticate=n;ts.forEachChild(n,visit);}visit(file);
test('auth policy checkbox starts unchecked with separate policy and disclaimer links',()=>{assert.ok(consent);const code=ts.transpileModule(`exports.view=(${consent.getText(file)});`,{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,module:ts.ModuleKind.CommonJS}}).outputText;const context=vm.createContext({exports:{},require,consent:false,setConsent(){}});vm.runInContext(code,context);const html=renderToStaticMarkup(context.exports.view);assert.match(html,/required/);assert.doesNotMatch(html,/checked/);assert.match(html,/href="\/privacy"/);assert.match(html,/href="\/terms"/);});
test('real authentication handler refuses to disconnect or submit before consent',async()=>{assert.ok(authenticate);let error;const context=vm.createContext({exports:{},auth:'login',busy:false,consent:false,policyVersion:'current',FormData:class{},setAuthError:value=>error=value,disconnect(){throw Error('must not disconnect');},api(){throw Error('must not call API');}});vm.runInContext(ts.transpileModule(`${authenticate.getText(file)};exports.authenticate=authenticate;`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText,context);await context.exports.authenticate({preventDefault(){},currentTarget:{}});assert.match(error,/请先阅读并同意/);});

const chat=await module('chat.ts');
test('reused TS client IDs cannot mix another identity into a private conversation',()=>{assert.equal(chat.chatVisible({scope:'client',from:2,peerUid:'old'},'private',2,'new'),false);assert.equal(chat.chatVisible({scope:'private',target:2,peerUid:'new'},'private',2,'new'),true);assert.equal(chat.chatVisible({scope:'system'},'private',2,'new'),false);});
